//! Sync-status and lifecycle types shared across the Chronos boundary.

use serde::{Deserialize, Serialize};

use crate::error::ChronosError;

/// Identifies which subsystem produced a time-sync status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeProvider {
    /// The kernel clock-discipline state, read via `adjtimex`.
    SystemClock,
    /// `chronyd`, queried over its command protocol.
    Chrony,
    /// The provider could not be determined.
    Unknown,
}

/// Whether a clock source is currently synchronized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    /// The source is disciplined and considered usable.
    Synchronized,
    /// The source is reachable but not currently disciplined.
    Unsynchronized,
    /// Synchronization state could not be determined.
    Unknown,
}

/// Domain view of a backend's clock discipline, independent of serialization.
///
/// The server's status provider yields this; the API layer maps it into a
/// [`TimeStatus`] for the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendStatus {
    /// The source is synchronized, optionally exposing stratum and last offset.
    Synchronized {
        /// NTP stratum reported by the source, if known.
        stratum: Option<u8>,
        /// Last clock offset from the source's tracking output, if known.
        last_offset_nanos: Option<i128>,
    },
    /// The source is not synchronized.
    Unsynchronized,
    /// The synchronization state could not be determined.
    Unknown,
}

/// Wire representation of a time-sync status, embedded in API responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeStatus {
    /// Which subsystem produced this status.
    pub provider: TimeProvider,
    /// Whether the source is synchronized.
    pub sync: SyncState,
    /// NTP stratum, when the provider exposes it.
    pub stratum: Option<u8>,
    /// Last clock offset in nanoseconds, when the provider exposes it.
    pub last_offset_nanos: Option<i128>,
}

impl TimeStatus {
    /// Builds a [`TimeStatus`] from a provider identity and a [`BackendStatus`].
    #[must_use]
    pub fn from_backend(provider: TimeProvider, status: BackendStatus) -> Self {
        match status {
            BackendStatus::Synchronized {
                stratum,
                last_offset_nanos,
            } => Self {
                provider,
                sync: SyncState::Synchronized,
                stratum,
                last_offset_nanos,
            },
            BackendStatus::Unsynchronized => Self {
                provider,
                sync: SyncState::Unsynchronized,
                stratum: None,
                last_offset_nanos: None,
            },
            BackendStatus::Unknown => Self {
                provider,
                sync: SyncState::Unknown,
                stratum: None,
                last_offset_nanos: None,
            },
        }
    }

    /// Returns whether this status represents a usable, synchronized source.
    #[must_use]
    pub fn is_synchronized(&self) -> bool {
        matches!(self.sync, SyncState::Synchronized)
    }
}

/// Lifecycle state of the gateway, surfaced on its status endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayState {
    /// The gateway has started but not yet completed a sampling round.
    Starting,
    /// A sampling round is in progress and no result is available yet.
    Sampling,
    /// The most recent round produced a good sample written to the backend.
    Synchronized,
    /// Recent rounds partially failed; the last good sample may be stale.
    Degraded,
    /// No usable samples are available and the source is not trusted.
    Unsynchronized,
}

/// A port that reports the local time-synchronization status of a host.
///
/// Implemented by outer crates (e.g. the `adjtimex`- and chrony-protocol-backed
/// providers in `chronos-server`); the domain depends only on this trait.
pub trait TimeStatusProvider {
    /// Returns which subsystem this provider represents.
    fn provider(&self) -> TimeProvider;

    /// Reads the current backend synchronization status.
    ///
    /// # Errors
    ///
    /// Returns [`ChronosError::TimeStatusUnavailable`] when the underlying
    /// source cannot be queried.
    fn backend_status(&self) -> Result<BackendStatus, ChronosError>;

    /// Reads the current status as a wire-ready [`TimeStatus`].
    ///
    /// # Errors
    ///
    /// Propagates any error from [`TimeStatusProvider::backend_status`].
    fn time_status(&self) -> Result<TimeStatus, ChronosError> {
        Ok(TimeStatus::from_backend(
            self.provider(),
            self.backend_status()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_synchronized_backend_to_time_status() {
        let status = TimeStatus::from_backend(
            TimeProvider::Chrony,
            BackendStatus::Synchronized {
                stratum: Some(3),
                last_offset_nanos: Some(12_000),
            },
        );
        assert!(status.is_synchronized());
        assert_eq!(status.stratum, Some(3));
        assert_eq!(status.last_offset_nanos, Some(12_000));
    }

    #[test]
    fn serializes_status_with_documented_field_names() {
        let status = TimeStatus {
            provider: TimeProvider::Chrony,
            sync: SyncState::Synchronized,
            stratum: Some(3),
            last_offset_nanos: Some(12_000),
        };
        let json = serde_json::to_value(&status).expect("serializable");
        assert_eq!(json["provider"], "chrony");
        assert_eq!(json["sync"], "synchronized");
        assert_eq!(json["stratum"], 3);
        assert_eq!(json["last_offset_nanos"], 12_000);
    }

    #[test]
    fn serializes_gateway_state_lowercase() {
        let json = serde_json::to_value(GatewayState::Synchronized).expect("serializable");
        assert_eq!(json, "synchronized");
    }

    #[test]
    fn unsynchronized_backend_clears_optional_fields() {
        let status = TimeStatus::from_backend(TimeProvider::Chrony, BackendStatus::Unsynchronized);
        assert_eq!(status.sync, SyncState::Unsynchronized);
        assert_eq!(status.stratum, None);
        assert_eq!(status.last_offset_nanos, None);
    }
}
