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
use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::backend_client::BackendClient;
use crate::config::GatewayConfig;
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
    #[arg(long, value_name = "FILE", default_value = "/etc/chronos/gateway.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = GatewayConfig::load(&cli.config)
        .with_context(|| format!("loading configuration from {}", cli.config.display()))?;
    init_tracing(&config.logging)?;
    tracing::info!(
        backends = config.backends.len(),
        sock_path = %config.chrony.sock_path.display(),
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

    let output: Arc<dyn OutputBackend + Send + Sync> = Arc::new(ChronySockRefclockBackend::new(
        config.chrony.sock_path.clone(),
    ));
    let status = SharedStatus::new(config.chrony.sock_path.display().to_string());

    serve_status(&config.status.listen, status.clone()).await?;

    Scheduler::new(clients, sampler, interval, output, status)
        .run()
        .await
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
