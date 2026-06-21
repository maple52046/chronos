//! `chronos-server` binary entry point: a thin composition root that parses the
//! CLI, loads configuration, initializes logging, and serves the Time API.

mod api;
mod config;
mod status_provider;
mod tls;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chronos_core::config::{LogFormat, LoggingConfig};
use chronos_core::TimeStatusProvider;
use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::api::{router, AppState};
use crate::config::ServerConfig;
use crate::status_provider::{
    ChronyStatusProvider, SystemClockStatusProvider, UnknownStatusProvider,
};

/// Command-line arguments for `chronos-server`.
#[derive(Debug, Parser)]
#[command(
    name = "chronos-server",
    version,
    about = "Chronos HTTP/HTTPS Time API server"
)]
struct Cli {
    /// Path to the YAML configuration file.
    #[arg(
        long,
        value_name = "FILE",
        global = true,
        default_value = "/etc/chronos/server.yaml"
    )]
    config: PathBuf,
    /// Optional subcommand; absent runs the server.
    #[command(subcommand)]
    command: Option<Command>,
}

/// `chronos-server` subcommands.
#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Probe the local health endpoint and exit non-zero if unhealthy.
    ///
    /// Intended as a container `HEALTHCHECK`, replacing a `curl` dependency.
    Healthcheck,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ensure_crypto_provider();
    let cli = Cli::parse();
    let config = ServerConfig::load(&cli.config)
        .with_context(|| format!("loading configuration from {}", cli.config.display()))?;

    if matches!(cli.command, Some(Command::Healthcheck)) {
        return run_healthcheck(&config);
    }

    init_tracing(&config.logging)?;

    let provider = build_provider(&config);
    let state = AppState::new(
        provider,
        config.api.cache_control.clone(),
        config.time_status.allow_unknown_status,
    );
    let app = router(state, &config.api.base_path);

    let addr: SocketAddr = config
        .server
        .listen
        .parse()
        .with_context(|| format!("parsing listen address {}", config.server.listen))?;
    let base_path = if config.api.base_path.is_empty() {
        "/"
    } else {
        &config.api.base_path
    };
    tracing::info!(%addr, tls = config.tls.enabled, base_path, "chronos-server listening");

    if config.tls.enabled {
        let cert_file = config
            .tls
            .cert_file
            .as_ref()
            .context("tls.enabled requires tls.cert_file")?;
        let key_file = config
            .tls
            .key_file
            .as_ref()
            .context("tls.enabled requires tls.key_file")?;
        tls::serve_https(addr, app, cert_file, key_file).await
    } else {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding {addr}"))?;
        axum::serve(listener, app).await.context("serving HTTP")?;
        Ok(())
    }
}

/// Selects a time-status provider based on configuration.
fn build_provider(config: &ServerConfig) -> Arc<dyn TimeStatusProvider + Send + Sync> {
    match config.time_status.provider.as_str() {
        "system" => Arc::new(SystemClockStatusProvider),
        "chrony" => Arc::new(ChronyStatusProvider::new(
            config.time_status.chrony_address.clone(),
        )),
        other => {
            tracing::warn!(
                provider = other,
                "unrecognized time_status provider; reporting unknown"
            );
            Arc::new(UnknownStatusProvider)
        }
    }
}

/// Probes the local health endpoint, returning an error when not healthy.
///
/// Operational context: this backs the container `HEALTHCHECK` without a `curl`
/// dependency. It issues a plain-HTTP request to the loopback listener, so it
/// assumes the reverse-proxy deployment where the server speaks HTTP locally.
fn run_healthcheck(config: &ServerConfig) -> anyhow::Result<()> {
    let addr: SocketAddr = config
        .server
        .listen
        .parse()
        .with_context(|| format!("parsing listen address {}", config.server.listen))?;
    let path = format!("{}/healthz", config.api.base_path);
    probe_healthz(addr.port(), &path)
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
/// Rationale: the workspace pins rustls to the ring backend and uses the
/// provider-less builders (via `axum-server`'s `tls-rustls-no-provider`), which
/// require a process-level default provider to be installed first. Guarded by
/// `Once` so it is safe to call from both the entry point and tests.
pub(crate) fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // An error means a provider is already installed, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
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
