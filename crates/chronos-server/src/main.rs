//! `chronos-server` binary entry point: a thin composition root that parses the
//! CLI, loads configuration, initializes logging, and serves the Time API.

mod api;
mod config;
mod status_provider;
mod tls;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use chronos_core::config::{LogFormat, LoggingConfig};
use chronos_core::TimeStatusProvider;
use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::api::{router, AppState};
use crate::config::ServerConfig;
use crate::status_provider::{ChronycStatusProvider, UnknownStatusProvider};

/// Command-line arguments for `chronos-server`.
#[derive(Debug, Parser)]
#[command(
    name = "chronos-server",
    version,
    about = "Chronos HTTP/HTTPS Time API server"
)]
struct Cli {
    /// Path to the YAML configuration file.
    #[arg(long, value_name = "FILE", default_value = "/etc/chronos/server.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = ServerConfig::load(&cli.config)
        .with_context(|| format!("loading configuration from {}", cli.config.display()))?;
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
        "chrony" => Arc::new(ChronycStatusProvider::new(
            config.time_status.chronyc_path.clone(),
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
