//! The crate's concrete, enumerated error type.

use thiserror::Error;

/// Errors produced by the Chronos domain logic and the ports it defines.
///
/// Outer crates map their transport- or driver-specific failures into these
/// variants so the domain and use cases never name a concrete adapter error.
#[derive(Debug, Error)]
pub enum ChronosError {
    /// A backend HTTP request did not complete successfully.
    #[error("backend request failed: {0}")]
    BackendRequest(String),

    /// TLS validation against the backend failed.
    #[error("TLS validation failed: {0}")]
    Tls(String),

    /// A backend response could not be parsed or violated the time-API contract.
    #[error("invalid backend response: {0}")]
    InvalidResponse(String),

    /// The backend reported that its own clock is not synchronized.
    #[error("backend is unsynchronized")]
    BackendUnsynchronized,

    /// A sampling round produced fewer good samples than required.
    #[error("insufficient good samples: got {got}, need {need}")]
    InsufficientSamples {
        /// Number of good samples actually collected in the round.
        got: usize,
        /// Minimum number of good samples the configuration requires.
        need: usize,
    },

    /// Writing a sample to the configured output backend failed.
    #[error("output backend error: {0}")]
    OutputBackend(String),

    /// The configured security policy rejected a backend transport.
    #[error("security policy rejected backend: {0}")]
    SecurityPolicy(String),

    /// Reading the time-status provider failed.
    #[error("time status unavailable: {0}")]
    TimeStatusUnavailable(String),
}
