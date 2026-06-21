//! `chronos-gateway` binary entry point: a thin composition root that parses the
//! CLI, loads configuration, initializes logging, samples backends, and feeds the
//! chrony SOCK refclock.

mod backend_client;
mod config;
mod filter;
mod sampler;
mod scheduler;
mod status_api;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chronos_chrony::ChronySockRefclockBackend;
use chronos_core::config::{LogFormat, LoggingConfig};
use chronos_core::OutputBackend;
use chronos_ntp::ShmRefclockBackend;
use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::backend_client::BackendClient;
use crate::config::{GatewayConfig, OutputConfig};
use crate::filter::RoundFilter;
use crate::sampler::Sampler;
use crate::scheduler::Scheduler;
use crate::status_api::SharedStatus;

/// Command-line arguments for `chronos-gateway`.
#[derive(Debug, Parser)]
#[command(
    name = "chronos-gateway",
    version,
    about = "Chronos gateway: samples a Time API backend and feeds chrony's SOCK refclock"
)]
struct Cli {
    /// Path to the YAML configuration file.
    #[arg(
        long,
        value_name = "FILE",
        global = true,
        default_value = "/etc/chronos/gateway.yaml"
    )]
    config: PathBuf,
    /// Optional subcommand; absent runs the gateway.
    #[command(subcommand)]
    command: Option<Command>,
}

/// `chronos-gateway` subcommands.
#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Probe the local status endpoint and exit non-zero if unhealthy.
    ///
    /// Intended as a container `HEALTHCHECK`, replacing a `curl` dependency.
    Healthcheck,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ensure_crypto_provider();
    let cli = Cli::parse();
    let config = GatewayConfig::load(&cli.config)
        .with_context(|| format!("loading configuration from {}", cli.config.display()))?;

    if matches!(cli.command, Some(Command::Healthcheck)) {
        return run_healthcheck(&config);
    }

    init_tracing(&config.logging)?;

    let output_config = config.resolve_output()?;
    if config.uses_deprecated_chrony_alias() {
        tracing::warn!(
            "`chrony:` config section is deprecated and will be removed; use `output: {{ type: chrony_sock, ... }}`"
        );
    }
    let output = build_output(&output_config)?;
    let output_target = output.target_description();
    tracing::info!(
        backends = config.backends.len(),
        output = %output_target,
        "chronos-gateway starting"
    );

    let policy = config.security.policy();
    let request_timeout =
        Duration::from_millis(config.sampling.max_rtt_ms.saturating_mul(4).max(1_000));
    let clients = build_clients(
        &config,
        policy,
        request_timeout,
        &config.security.pinned_spki,
    )?;
    for client in &clients {
        tracing::debug!(backend = client.name(), url = %client.url(), transport = ?client.transport(), "backend client ready");
    }

    let filter = RoundFilter {
        max_rtt_nanos: config.sampling.max_rtt_ms.saturating_mul(1_000_000),
        outlier_threshold_nanos: config
            .sampling
            .outlier_threshold_ms
            .saturating_mul(1_000_000),
    };
    let sampler = Sampler::new(
        config.sampling.burst_samples,
        config.sampling.min_good_samples,
        filter,
    );
    let interval = Duration::from_secs(config.sampling.interval_seconds);

    let status = SharedStatus::new(output_kind(&output_config), output_target);

    serve_status(&config.status.listen, status.clone()).await?;

    Scheduler::new(clients, sampler, interval, output, status)
        .run()
        .await
}

/// Builds the configured output backend, selecting the implementation by the
/// `output` discriminant (the composition root's only place that names a
/// concrete adapter).
fn build_output(output: &OutputConfig) -> anyhow::Result<Arc<dyn OutputBackend + Send + Sync>> {
    match output {
        OutputConfig::ChronySock(chrony) => Ok(Arc::new(ChronySockRefclockBackend::new(
            chrony.sock_path.clone(),
        ))),
        OutputConfig::NtpShm(shm) => {
            let backend = ShmRefclockBackend::new(shm.unit, shm.perm_bits()?, shm.precision)?;
            Ok(Arc::new(backend))
        }
    }
}

/// Returns the stable status discriminant string for an output backend.
fn output_kind(output: &OutputConfig) -> &'static str {
    match output {
        OutputConfig::ChronySock(_) => "chrony_sock",
        OutputConfig::NtpShm(_) => "ntp_shm",
    }
}

/// Probes the local status endpoint, returning an error when not healthy.
///
/// Operational context: this backs the container `HEALTHCHECK` without a `curl`
/// dependency, issuing a plain-HTTP request to the loopback status listener.
fn run_healthcheck(config: &GatewayConfig) -> anyhow::Result<()> {
    let addr: SocketAddr = config
        .status
        .listen
        .parse()
        .with_context(|| format!("parsing status listen address {}", config.status.listen))?;
    probe_healthz(addr.port(), "/healthz")
}

/// Issues a minimal HTTP/1.0 GET to `http://127.0.0.1:<port><path>`.
///
/// Returns an error unless the response status is `200`.
fn probe_healthz(port: u16, path: &str) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(&addr).with_context(|| format!("connecting to {addr}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;

    let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .context("sending health request")?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .context("reading health response")?;
    let head = String::from_utf8_lossy(&buf);
    let status_line = head.lines().next().unwrap_or_default();
    if status_line.split_whitespace().nth(1) == Some("200") {
        Ok(())
    } else {
        anyhow::bail!("health endpoint not healthy: {status_line:?}")
    }
}

/// Installs the ring `CryptoProvider` as the process default for rustls, once.
///
/// Rationale: the workspace pins rustls to the ring backend and reqwest's
/// `rustls-no-provider` feature requires a process-level default provider to be
/// installed before any client is built. Guarded by `Once` so it is safe to
/// call from both the binary entry point and each client constructor.
pub(crate) fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // An error means a provider is already installed, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Binds and spawns the local status HTTP server.
async fn serve_status(listen: &str, status: SharedStatus) -> anyhow::Result<()> {
    let addr: SocketAddr = listen
        .parse()
        .with_context(|| format!("parsing status listen address {listen}"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding status endpoint {addr}"))?;
    tracing::info!(%addr, "chronos-gateway status endpoint listening");
    let router = status_api::router(status);
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, router).await {
            tracing::error!(error = %err, "status endpoint terminated");
        }
    });
    Ok(())
}

/// Builds one [`BackendClient`] per configured backend, enforcing the policy.
fn build_clients(
    config: &GatewayConfig,
    policy: chronos_core::SecurityPolicy,
    request_timeout: Duration,
    pinned_spki: &[String],
) -> anyhow::Result<Vec<BackendClient>> {
    config
        .backends
        .iter()
        .map(|backend| BackendClient::new(backend, policy, request_timeout, pinned_spki))
        .collect()
}

/// Initializes the global tracing subscriber from logging configuration.
///
/// `RUST_LOG` overrides the configured level when present so operators can raise
/// verbosity without editing the config file.
fn init_tracing(logging: &LoggingConfig) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&logging.level))
        .context("building tracing filter")?;
    let registry = tracing_subscriber::registry().with(filter);
    match logging.format {
        LogFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().json())
            .try_init()
            .context("initializing JSON logging")?,
        LogFormat::Text => registry
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .context("initializing text logging")?,
    }
    Ok(())
}
