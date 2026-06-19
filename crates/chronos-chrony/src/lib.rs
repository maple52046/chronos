//! chrony SOCK refclock output backend for Chronos.
//!
//! This crate implements the [`chronos_core::OutputBackend`] port by encoding
//! samples into chrony's `sock_sample` wire format and sending them over a Unix
//! datagram socket. It performs no clock adjustment of its own.
#![forbid(unsafe_code)]

pub mod sock_refclock;
pub mod writer;

pub use sock_refclock::{SockSample, SOCK_MAGIC, SOCK_SAMPLE_LEN};
pub use writer::ChronySockRefclockBackend;
