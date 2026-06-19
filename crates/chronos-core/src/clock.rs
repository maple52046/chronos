//! Clock ports.
//!
//! The domain measures durations and reads wall-clock time only through these
//! traits, so use cases stay testable with fake clocks and never depend on
//! `std::time` directly.

/// A port for reading a strictly non-decreasing clock used to measure durations.
///
/// Implementations wrap a monotonic source such as [`std::time::Instant`]. The
/// epoch is unspecified; only differences between readings are meaningful.
pub trait MonotonicClock {
    /// Returns the current monotonic reading in nanoseconds since an
    /// implementation-defined, fixed epoch.
    fn now_nanos(&self) -> u128;
}

/// A port for reading wall-clock time as Unix nanoseconds.
///
/// Implementations wrap a real-time source such as
/// [`std::time::SystemTime`]. The value may jump backwards if the system clock
/// is adjusted; callers must not assume monotonicity.
pub trait WallClock {
    /// Returns the current wall-clock time in nanoseconds since the Unix epoch.
    fn now_unix_nanos(&self) -> i128;
}
