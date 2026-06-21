//! ntpd/ntpsec SHM refclock output backend for Chronos.
//!
//! This crate implements the [`chronos_core::OutputBackend`] port by writing
//! samples into the SysV shared-memory segment that ntpd/ntpsec's `SHM(u)`
//! refclock driver (`127.127.28.u`) reads. It performs no clock adjustment of
//! its own; the local NTP daemon remains the sole clock disciplinarian.
//!
//! When the segment is created with world-writable permissions (the ntpd
//! convention for unit numbers `>= 2`), the gateway can feed it without running
//! as root.

pub mod shm_refclock;
pub mod writer;

pub use shm_refclock::{ShmTime, NTP_SHM_KEY_BASE, SHM_TIME_SIZE};
pub use writer::ShmRefclockBackend;
