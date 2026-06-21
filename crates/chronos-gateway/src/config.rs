//! `chronos-gateway` configuration model and YAML loading.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use base64::prelude::Engine as _;
use chronos_core::config::LoggingConfig;
use chronos_core::SecurityPolicy;
use serde::{Deserialize, Serialize};

/// Top-level `chronos-gateway` configuration, parsed from a YAML file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayConfig {
    /// Ordered list of time backends; earlier entries are preferred.
    pub backends: Vec<BackendConfig>,
    /// Burst-sampling parameters.
    #[serde(default)]
    pub sampling: SamplingConfig,
    /// chrony SOCK refclock output configuration.
    pub chrony: ChronyConfig,
    /// Backend transport security policy.
    #[serde(default)]
    pub security: SecurityConfig,
    /// Local status HTTP endpoint configuration.
    #[serde(default)]
    pub status: StatusConfig,
    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// A single time backend the gateway may sample.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackendConfig {
    /// Operator-facing identifier used in logs and the status endpoint.
    pub name: String,
    /// Base URL of the Chronos server, including any path prefix but not the
    /// endpoint, e.g. `https://time.example.com` or
    /// `https://time.example.com/chronos`. The gateway appends `/time`.
    pub base_url: String,
    /// Whether this backend must be reached over TLS.
    #[serde(default = "default_true")]
    pub require_tls: bool,
    /// Whether the TLS certificate chain and hostname must validate.
    #[serde(default = "default_true")]
    pub require_valid_cert: bool,
}

/// Burst-sampling parameters controlling how each round is collected and filtered.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SamplingConfig {
    /// Seconds between the start of consecutive sampling rounds.
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u64,
    /// Number of requests issued per sampling round.
    #[serde(default = "default_burst_samples")]
    pub burst_samples: u32,
    /// Minimum number of good samples required to accept a round.
    #[serde(default = "default_min_good_samples")]
    pub min_good_samples: u32,
    /// Round-trip-time ceiling, in milliseconds, above which a sample is rejected.
    #[serde(default = "default_max_rtt_ms")]
    pub max_rtt_ms: u64,
    /// Maximum allowed deviation, in milliseconds, from the round median offset.
    #[serde(default = "default_outlier_threshold_ms")]
    pub outlier_threshold_ms: u64,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            interval_seconds: default_interval_seconds(),
            burst_samples: default_burst_samples(),
            min_good_samples: default_min_good_samples(),
            max_rtt_ms: default_max_rtt_ms(),
            outlier_threshold_ms: default_outlier_threshold_ms(),
        }
    }
}

/// chrony SOCK refclock output configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChronyConfig {
    /// Path of the chrony SOCK refclock Unix datagram socket.
    pub sock_path: PathBuf,
    /// chrony reference identifier; matches `refid` in `chrony.conf`.
    #[serde(default = "default_refid")]
    pub refid: String,
}

/// Backend transport security policy.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    /// Whether remote plain-HTTP backends are permitted at all.
    #[serde(default)]
    pub allow_plain_http_backends: bool,
    /// Whether plain-HTTP is permitted specifically for loopback backends.
    #[serde(default = "default_true")]
    pub allow_plain_http_loopback: bool,
    /// Base64 SPKI hashes to pin; empty disables pinning.
    #[serde(default)]
    pub pinned_spki: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            allow_plain_http_backends: false,
            allow_plain_http_loopback: true,
            pinned_spki: Vec::new(),
        }
    }
}

impl SecurityConfig {
    /// Projects this configuration onto the core transport [`SecurityPolicy`].
    #[must_use]
    pub fn policy(&self) -> SecurityPolicy {
        SecurityPolicy {
            allow_plain_http_backends: self.allow_plain_http_backends,
            allow_plain_http_loopback: self.allow_plain_http_loopback,
        }
    }
}

/// Local status HTTP endpoint configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusConfig {
    /// Socket address the local status endpoint binds to.
    #[serde(default = "default_status_listen")]
    pub listen: String,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            listen: default_status_listen(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_interval_seconds() -> u64 {
    30
}

fn default_burst_samples() -> u32 {
    5
}

fn default_min_good_samples() -> u32 {
    3
}

fn default_max_rtt_ms() -> u64 {
    300
}

fn default_outlier_threshold_ms() -> u64 {
    100
}

fn default_refid() -> String {
    "CHRO".to_string()
}

fn default_status_listen() -> String {
    "127.0.0.1:9090".to_string()
}

impl GatewayConfig {
    /// Loads and validates a [`GatewayConfig`] from a YAML file at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the YAML cannot be parsed,
    /// or the resulting configuration fails [`GatewayConfig::validate`].
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let config: Self = serde_yaml::from_str(&raw)
            .with_context(|| format!("parsing YAML config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Checks cross-field invariants that `serde` cannot express on its own.
    ///
    /// # Errors
    ///
    /// Returns an error when no backends are configured or when
    /// `min_good_samples` exceeds `burst_samples`.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.backends.is_empty() {
            anyhow::bail!("at least one backend must be configured");
        }
        if self.sampling.min_good_samples > self.sampling.burst_samples {
            anyhow::bail!(
                "sampling.min_good_samples ({}) must not exceed sampling.burst_samples ({})",
                self.sampling.min_good_samples,
                self.sampling.burst_samples
            );
        }
        for pin in &self.security.pinned_spki {
            let decoded = base64::prelude::BASE64_STANDARD
                .decode(pin)
                .map_err(|err| {
                    anyhow::anyhow!("security.pinned_spki entry {pin:?} is not valid base64: {err}")
                })?;
            if decoded.len() != 32 {
                anyhow::bail!(
                    "security.pinned_spki entry {pin:?} must be a base64 SHA-256 (32 bytes), got {} bytes",
                    decoded.len()
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_production_config_with_defaults() {
        let yaml = r#"
backends:
  - name: "primary"
    base_url: "https://time.example.com"
    require_tls: true
    require_valid_cert: true
sampling:
  interval_seconds: 30
  burst_samples: 5
  min_good_samples: 3
  max_rtt_ms: 300
  outlier_threshold_ms: 100
chrony:
  sock_path: "/run/chrony/chronos.sock"
  refid: "CHRO"
security:
  allow_plain_http_backends: false
  allow_plain_http_loopback: true
  pinned_spki: []
status:
  listen: "127.0.0.1:9090"
logging:
  level: "info"
  format: "json"
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.backends.len(), 1);
        assert_eq!(cfg.chrony.refid, "CHRO");
        cfg.validate().expect("valid config");
    }

    #[test]
    fn lab_config_applies_defaults() {
        let yaml = r#"
backends:
  - name: "lab"
    base_url: "http://192.168.100.10:8080"
    require_tls: false
    require_valid_cert: false
chrony:
  sock_path: "/run/chrony/chronos.sock"
security:
  allow_plain_http_backends: true
  allow_plain_http_loopback: true
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.sampling.burst_samples, 5);
        assert_eq!(cfg.chrony.refid, "CHRO");
        assert_eq!(cfg.status.listen, "127.0.0.1:9090");
        cfg.validate().expect("valid config");
    }

    #[test]
    fn accepts_valid_spki_pin() {
        let yaml = r#"
backends:
  - name: "primary"
    base_url: "https://time.example.com"
chrony:
  sock_path: "/run/chrony/chronos.sock"
security:
  pinned_spki:
    - "ToXX6YsrFz2DC4I6K/IxLdW9np+HHirOUAfxobC/jCI="
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        cfg.validate().expect("valid config");
    }

    #[test]
    fn rejects_malformed_spki_pin() {
        let yaml = r#"
backends:
  - name: "primary"
    base_url: "https://time.example.com"
chrony:
  sock_path: "/run/chrony/chronos.sock"
security:
  pinned_spki:
    - "not-a-valid-pin"
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_min_good_samples_above_burst() {
        let yaml = r#"
backends:
  - name: "primary"
    base_url: "https://time.example.com"
sampling:
  burst_samples: 3
  min_good_samples: 5
chrony:
  sock_path: "/run/chrony/chronos.sock"
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert!(cfg.validate().is_err());
    }
}
