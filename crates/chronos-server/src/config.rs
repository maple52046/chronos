//! `chronos-server` configuration model and YAML loading.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chronos_core::config::LoggingConfig;
use serde::{Deserialize, Serialize};

/// Top-level `chronos-server` configuration, parsed from a YAML file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// Listener socket configuration.
    pub server: ListenerConfig,
    /// Native TLS configuration; disabled by default for reverse-proxy setups.
    #[serde(default)]
    pub tls: TlsConfig,
    /// HTTP API behavior knobs.
    #[serde(default)]
    pub api: ApiConfig,
    /// Time-status provider configuration.
    #[serde(default)]
    pub time_status: TimeStatusConfig,
    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Network listener configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListenerConfig {
    /// Socket address the HTTP/HTTPS server binds to, e.g. `127.0.0.1:8080`.
    pub listen: String,
}

/// Native TLS configuration for the HTTPS transport mode.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TlsConfig {
    /// Whether the server terminates TLS itself rather than behind a proxy.
    #[serde(default)]
    pub enabled: bool,
    /// PEM certificate chain path; required when [`TlsConfig::enabled`] is true.
    #[serde(default)]
    pub cert_file: Option<PathBuf>,
    /// PEM private key path; required when [`TlsConfig::enabled`] is true.
    #[serde(default)]
    pub key_file: Option<PathBuf>,
}

/// HTTP API behavior configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiConfig {
    /// Value sent in the `Cache-Control` response header for time endpoints.
    #[serde(default = "default_cache_control")]
    pub cache_control: String,
    /// Path prefix the API mounts under, e.g. `/chronos`; empty serves at root.
    ///
    /// Lets the server share one reverse proxy with other services. The value
    /// is normalized by [`ApiConfig::normalize_base_path`] before use.
    #[serde(default)]
    pub base_path: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            cache_control: default_cache_control(),
            base_path: String::new(),
        }
    }
}

impl ApiConfig {
    /// Rewrites `base_path` into its canonical form: empty, or a single leading
    /// slash with no trailing slash (e.g. `/chronos`).
    ///
    /// # Errors
    ///
    /// Returns an error when the prefix contains whitespace or empty path
    /// segments (e.g. `//`), which would produce ambiguous routes.
    fn normalize_base_path(&mut self) -> anyhow::Result<()> {
        let trimmed = self.base_path.trim_matches('/');
        if trimmed.is_empty() {
            self.base_path = String::new();
            return Ok(());
        }
        if trimmed.split('/').any(|segment| segment.is_empty()) {
            anyhow::bail!(
                "api.base_path {:?} must not contain empty segments",
                self.base_path
            );
        }
        if trimmed.chars().any(char::is_whitespace) {
            anyhow::bail!(
                "api.base_path {:?} must not contain whitespace",
                self.base_path
            );
        }
        self.base_path = format!("/{trimmed}");
        Ok(())
    }
}

fn default_cache_control() -> String {
    "no-store".to_string()
}

/// Time-status provider configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeStatusConfig {
    /// Provider identifier; `chrony` shells out to `chronyc`, others are inert.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Path to the `chronyc` binary used by the chrony provider.
    #[serde(default = "default_chronyc_path")]
    pub chronyc_path: PathBuf,
    /// Whether an `unknown` sync status is acceptable when reporting health.
    #[serde(default)]
    pub allow_unknown_status: bool,
}

impl Default for TimeStatusConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            chronyc_path: default_chronyc_path(),
            allow_unknown_status: false,
        }
    }
}

fn default_provider() -> String {
    "chrony".to_string()
}

fn default_chronyc_path() -> PathBuf {
    PathBuf::from("/usr/bin/chronyc")
}

impl ServerConfig {
    /// Loads and validates a [`ServerConfig`] from a YAML file at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the YAML cannot be parsed,
    /// or the resulting configuration fails [`ServerConfig::validate`].
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let mut config: Self = serde_yaml::from_str(&raw)
            .with_context(|| format!("parsing YAML config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Checks cross-field invariants and normalizes the API base path.
    ///
    /// # Errors
    ///
    /// Returns an error when TLS is enabled without both a certificate and key,
    /// or when `api.base_path` is malformed.
    pub fn validate(&mut self) -> anyhow::Result<()> {
        if self.tls.enabled && (self.tls.cert_file.is_none() || self.tls.key_file.is_none()) {
            anyhow::bail!("tls.enabled requires both tls.cert_file and tls.key_file");
        }
        self.api.normalize_base_path()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_native_http_config() {
        let yaml = r#"
server:
  listen: "127.0.0.1:8080"
tls:
  enabled: false
api:
  cache_control: "no-store"
time_status:
  provider: "chrony"
  chronyc_path: "/usr/bin/chronyc"
  allow_unknown_status: false
logging:
  level: "info"
  format: "json"
"#;
        let mut cfg: ServerConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.server.listen, "127.0.0.1:8080");
        assert!(!cfg.tls.enabled);
        assert_eq!(cfg.api.cache_control, "no-store");
        cfg.validate().expect("valid config");
        assert_eq!(cfg.api.base_path, "");
    }

    #[test]
    fn rejects_tls_without_cert_and_key() {
        let yaml = r#"
server:
  listen: "0.0.0.0:8443"
tls:
  enabled: true
"#;
        let mut cfg: ServerConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn normalizes_base_path_variants() {
        for raw in ["/chronos", "chronos", "/chronos/", "chronos/"] {
            let yaml =
                format!("server:\n  listen: \"127.0.0.1:8080\"\napi:\n  base_path: \"{raw}\"\n");
            let mut cfg: ServerConfig = serde_yaml::from_str(&yaml).expect("valid yaml");
            cfg.validate().expect("valid config");
            assert_eq!(cfg.api.base_path, "/chronos", "input {raw:?}");
        }
    }

    #[test]
    fn rejects_base_path_with_empty_segment() {
        let yaml = r#"
server:
  listen: "127.0.0.1:8080"
api:
  base_path: "/chronos//time"
"#;
        let mut cfg: ServerConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert!(cfg.validate().is_err());
    }
}
