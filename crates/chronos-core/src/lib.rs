//! Domain types, ports, and pure time-synchronization logic for Chronos.
//!
//! This crate is the innermost layer of the Clean Architecture used by the
//! project: it owns the data that crosses boundaries and the trait contracts
//! (ports) that outer crates implement. It deliberately depends only on
//! `serde`/`serde_json`/`thiserror` — never on an async runtime, an HTTP stack,
//! `reqwest`, or chrony — so the synchronization logic stays unit-testable
//! without I/O.
#![forbid(unsafe_code)]

pub mod clock;
pub mod config;
pub mod error;
pub mod estimate;
pub mod sample;
pub mod status;

pub use clock::{MonotonicClock, WallClock};
pub use config::{BackendTransport, LogFormat, LoggingConfig, SecurityPolicy};
pub use error::ChronosError;
pub use sample::{OutputBackend, SampleQuality, TimeSample};
pub use status::{
    BackendStatus, GatewayState, SyncState, TimeProvider, TimeStatus, TimeStatusProvider,
};
