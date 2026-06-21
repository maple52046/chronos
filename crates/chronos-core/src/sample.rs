//! Time-sample types and the output-backend port.

use serde::{Deserialize, Serialize};

use crate::error::ChronosError;

/// Classification of a single collected sample, used for filtering and logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleQuality {
    /// The sample passed every filter and is eligible for selection.
    Good,
    /// The HTTP request to the backend failed.
    HttpError,
    /// TLS validation against the backend failed.
    TlsError,
    /// The response was missing or violated the time-API contract.
    InvalidResponse,
    /// The backend reported that its own clock is unsynchronized.
    BackendUnsynchronized,
    /// The round-trip time exceeded the configured ceiling.
    HighLatency,
    /// The sample deviated too far from the round's median offset.
    Outlier,
    /// The round did not yield enough good samples to be usable.
    InsufficientSamples,
}

impl SampleQuality {
    /// Returns whether this sample is usable for offset selection.
    #[must_use]
    pub fn is_good(&self) -> bool {
        matches!(self, SampleQuality::Good)
    }
}

/// A single time measurement against a backend.
///
/// All timestamps are nanoseconds. `i128` keeps Unix-nanosecond arithmetic
/// exact and signed (offsets can be negative) without overflow concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSample {
    /// Name of the backend this sample was taken from.
    pub backend_name: String,
    /// Server send timestamp from the response, in Unix nanoseconds.
    pub server_send_unix_nanos: i128,
    /// Local wall-clock receive timestamp, in Unix nanoseconds.
    pub local_receive_unix_nanos: i128,
    /// Measured round-trip time, in nanoseconds.
    pub rtt_nanos: u64,
    /// Estimated local-clock offset relative to the backend, in nanoseconds.
    pub estimated_offset_nanos: i128,
    /// Quality classification assigned by the filter.
    pub quality: SampleQuality,
}

/// A port that accepts good time samples and forwards them to a time backend.
///
/// The v1 implementation (`chronos-chrony`) writes to chrony's SOCK refclock.
/// v2 backends (built-in NTP server, direct clock setter) implement the same
/// trait without changing the domain.
pub trait OutputBackend {
    /// Submits a single accepted sample to the backend.
    ///
    /// # Errors
    ///
    /// Returns [`ChronosError::OutputBackend`] when the sample cannot be
    /// delivered (e.g. the chrony socket is unavailable).
    fn submit_sample(&self, sample: &TimeSample) -> Result<(), ChronosError>;

    /// Returns a short, operator-facing description of the output target.
    ///
    /// Rationale: the status endpoint reports which sink samples are written to
    /// without naming a concrete adapter. This is pure introspection; the domain
    /// owns the contract while outer crates supply the backend-specific text.
    fn target_description(&self) -> String {
        "unknown output backend".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_quality_is_usable() {
        assert!(SampleQuality::Good.is_good());
        assert!(!SampleQuality::HighLatency.is_good());
    }

    #[test]
    fn sample_quality_serializes_as_variant_name() {
        let json = serde_json::to_value(SampleQuality::Good).expect("serializable");
        assert_eq!(json, "Good");
    }
}
