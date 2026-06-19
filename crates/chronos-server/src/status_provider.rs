//! Time-status providers for `chronos-server`.
//!
//! The chrony provider shells out to `chronyc tracking` and parses its output.
//! Parsing is split into a pure [`parse_tracking`] function so it can be tested
//! without a running `chronyd`.

use std::path::PathBuf;
use std::process::Command;

use chronos_core::{BackendStatus, ChronosError, TimeProvider, TimeStatusProvider};

/// A [`TimeStatusProvider`] that queries `chronyd` via the `chronyc` CLI.
#[derive(Debug, Clone)]
pub struct ChronycStatusProvider {
    chronyc_path: PathBuf,
}

impl ChronycStatusProvider {
    /// Creates a provider that invokes the `chronyc` binary at `chronyc_path`.
    #[must_use]
    pub fn new(chronyc_path: PathBuf) -> Self {
        Self { chronyc_path }
    }
}

impl TimeStatusProvider for ChronycStatusProvider {
    fn provider(&self) -> TimeProvider {
        TimeProvider::Chrony
    }

    fn backend_status(&self) -> Result<BackendStatus, ChronosError> {
        let output = Command::new(&self.chronyc_path)
            .arg("tracking")
            .output()
            .map_err(|err| {
                ChronosError::TimeStatusUnavailable(format!(
                    "spawning {}: {err}",
                    self.chronyc_path.display()
                ))
            })?;
        if !output.status.success() {
            return Err(ChronosError::TimeStatusUnavailable(format!(
                "chronyc exited with {}",
                output.status
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(parse_tracking(&text))
    }
}

/// A provider that always reports an unknown status.
///
/// Selected when the configured provider is neither `chrony` nor otherwise
/// recognized, so the server still serves time without claiming a sync state.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnknownStatusProvider;

impl TimeStatusProvider for UnknownStatusProvider {
    fn provider(&self) -> TimeProvider {
        TimeProvider::Unknown
    }

    fn backend_status(&self) -> Result<BackendStatus, ChronosError> {
        Ok(BackendStatus::Unknown)
    }
}

/// Parses `chronyc tracking` text output into a [`BackendStatus`].
///
/// The `Leap status` line is authoritative for synchronization: `Normal` (or an
/// announced leap second) means synchronized, `Not synchronised` means not, and
/// anything else is treated as unknown. Stratum and last offset are extracted
/// when present.
#[must_use]
pub fn parse_tracking(output: &str) -> BackendStatus {
    let mut stratum: Option<u8> = None;
    let mut last_offset_nanos: Option<i128> = None;
    let mut leap: Option<String> = None;

    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "Stratum" => stratum = value.trim().parse::<u8>().ok(),
            "Last offset" => last_offset_nanos = parse_seconds_to_nanos(value.trim()),
            "Leap status" => leap = Some(value.trim().to_string()),
            _ => {}
        }
    }

    match classify_leap(leap.as_deref()) {
        Some(true) => BackendStatus::Synchronized {
            stratum,
            last_offset_nanos,
        },
        Some(false) => BackendStatus::Unsynchronized,
        None => BackendStatus::Unknown,
    }
}

/// Maps a chrony `Leap status` value to a synchronized/unsynchronized/unknown decision.
fn classify_leap(leap: Option<&str>) -> Option<bool> {
    let value = leap?.to_ascii_lowercase();
    if value.contains("not synchron") {
        Some(false)
    } else if value.contains("normal") || value.contains("second") {
        Some(true)
    } else {
        None
    }
}

/// Parses a leading seconds value (e.g. `+0.000012000`) into nanoseconds.
fn parse_seconds_to_nanos(value: &str) -> Option<i128> {
    let token = value.split_whitespace().next()?;
    let seconds: f64 = token.parse().ok()?;
    Some((seconds * 1e9).round() as i128)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNCED_TRACKING: &str = "\
Reference ID    : C0A86401 (time.example.com)
Stratum         : 3
Ref time (UTC)  : Fri Jun 19 14:00:00 2026
System time     : 0.000000123 seconds slow of NTP time
Last offset     : +0.000012000 seconds
RMS offset      : 0.000050000 seconds
Frequency       : 1.234 ppm slow
Residual freq   : +0.001 ppm
Skew            : 0.500 ppm
Root delay      : 0.001000000 seconds
Root dispersion : 0.000500000 seconds
Update interval : 64.0 seconds
Leap status     : Normal
";

    const UNSYNCED_TRACKING: &str = "\
Reference ID    : 00000000 ()
Stratum         : 0
Ref time (UTC)  : Thu Jan 01 00:00:00 1970
System time     : 0.000000000 seconds fast of NTP time
Last offset     : +0.000000000 seconds
Leap status     : Not synchronised
";

    #[test]
    fn parses_synchronized_tracking() {
        let status = parse_tracking(SYNCED_TRACKING);
        assert_eq!(
            status,
            BackendStatus::Synchronized {
                stratum: Some(3),
                last_offset_nanos: Some(12_000),
            }
        );
    }

    #[test]
    fn parses_unsynchronized_tracking() {
        assert_eq!(
            parse_tracking(UNSYNCED_TRACKING),
            BackendStatus::Unsynchronized
        );
    }

    #[test]
    fn empty_output_is_unknown() {
        assert_eq!(parse_tracking(""), BackendStatus::Unknown);
    }

    #[test]
    fn parses_negative_offset() {
        let text = "Stratum : 2\nLast offset : -0.000005000 seconds\nLeap status : Normal\n";
        assert_eq!(
            parse_tracking(text),
            BackendStatus::Synchronized {
                stratum: Some(2),
                last_offset_nanos: Some(-5_000),
            }
        );
    }
}
