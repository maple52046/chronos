//! Configuration value types shared by the Chronos binaries.
//!
//! These types are pure data with `serde` derives. The domain layer never reads
//! files or selects a logging backend; binaries parse YAML into these structs and
//! apply them at the composition root.

use serde::{Deserialize, Serialize};

use crate::error::ChronosError;

/// Log record output format selected from configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable single-line text, suited to interactive and lab use.
    Text,
    /// Structured JSON, one object per line, suited to log shipping in production.
    #[default]
    Json,
}

/// Logging configuration shared by `chronos-server` and `chronos-gateway`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Tracing filter directive, e.g. `info` or `chronos_gateway=debug,info`.
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Output format for emitted log records.
    #[serde(default)]
    pub format: LogFormat,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: LogFormat::default(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Classification of a backend's transport, derived from its URL by the caller.
///
/// The gateway parses the backend URL (which requires a URL library that the
/// domain does not depend on) and reduces it to one of these cases before
/// asking the [`SecurityPolicy`] to rule on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTransport {
    /// An `https://` backend.
    Https,
    /// A plain `http://` backend whose host is a loopback address.
    PlainHttpLoopback,
    /// A plain `http://` backend whose host is not loopback.
    PlainHttpRemote,
}

/// The gateway's backend transport security policy.
///
/// This is pure decision logic over configuration flags; the gateway enforces
/// it at the edge before issuing any request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityPolicy {
    /// Whether remote plain-HTTP backends are permitted at all.
    pub allow_plain_http_backends: bool,
    /// Whether plain-HTTP is permitted specifically for loopback backends.
    pub allow_plain_http_loopback: bool,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            allow_plain_http_backends: false,
            allow_plain_http_loopback: true,
        }
    }
}

impl SecurityPolicy {
    /// Rules on whether a classified backend transport is allowed.
    ///
    /// HTTPS is always allowed. Plain-HTTP loopback is allowed only when
    /// [`SecurityPolicy::allow_plain_http_loopback`] is set, and plain-HTTP to a
    /// remote host only when [`SecurityPolicy::allow_plain_http_backends`] is set.
    ///
    /// # Errors
    ///
    /// Returns [`ChronosError::SecurityPolicy`] when the transport is forbidden
    /// by the configured flags.
    pub fn evaluate(&self, transport: BackendTransport) -> Result<(), ChronosError> {
        match transport {
            BackendTransport::Https => Ok(()),
            BackendTransport::PlainHttpLoopback => {
                if self.allow_plain_http_loopback {
                    Ok(())
                } else {
                    Err(ChronosError::SecurityPolicy(
                        "plain-HTTP loopback backend is disabled by policy".to_string(),
                    ))
                }
            }
            BackendTransport::PlainHttpRemote => {
                if self.allow_plain_http_backends {
                    Ok(())
                } else {
                    Err(ChronosError::SecurityPolicy(
                        "remote plain-HTTP backend is disabled by policy".to_string(),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_config_defaults_to_info_json() {
        let cfg = LoggingConfig::default();
        assert_eq!(cfg.level, "info");
        assert_eq!(cfg.format, LogFormat::Json);
    }

    #[test]
    fn log_format_deserializes_lowercase() {
        let text: LogFormat = serde_json::from_str("\"text\"").expect("valid format");
        assert_eq!(text, LogFormat::Text);
        let json: LogFormat = serde_json::from_str("\"json\"").expect("valid format");
        assert_eq!(json, LogFormat::Json);
    }

    #[test]
    fn policy_always_allows_https() {
        let policy = SecurityPolicy {
            allow_plain_http_backends: false,
            allow_plain_http_loopback: false,
        };
        assert!(policy.evaluate(BackendTransport::Https).is_ok());
    }

    #[test]
    fn default_policy_allows_loopback_but_rejects_remote_http() {
        let policy = SecurityPolicy::default();
        assert!(policy.evaluate(BackendTransport::PlainHttpLoopback).is_ok());
        assert!(policy.evaluate(BackendTransport::PlainHttpRemote).is_err());
    }

    #[test]
    fn loopback_http_rejected_when_disabled() {
        let policy = SecurityPolicy {
            allow_plain_http_backends: false,
            allow_plain_http_loopback: false,
        };
        assert!(policy
            .evaluate(BackendTransport::PlainHttpLoopback)
            .is_err());
    }

    #[test]
    fn remote_http_allowed_when_enabled() {
        let policy = SecurityPolicy {
            allow_plain_http_backends: true,
            allow_plain_http_loopback: true,
        };
        assert!(policy.evaluate(BackendTransport::PlainHttpRemote).is_ok());
    }
}
