//! Pure offset-estimation, round-trip-time, median, and outlier helpers.
//!
//! These functions contain the numerical heart of the gateway and are written
//! to be unit-testable without a clock, runtime, or network.

/// Computes round-trip time from two monotonic-clock readings, in nanoseconds.
///
/// `t0` is taken before the request and `t3` after the response. A monotonic
/// clock never goes backwards, so `t3 >= t0`; the subtraction saturates at zero
/// to stay defensive against a misbehaving clock source.
#[must_use]
pub fn rtt_nanos(t0_monotonic_nanos: u128, t3_monotonic_nanos: u128) -> u64 {
    let delta = t3_monotonic_nanos.saturating_sub(t0_monotonic_nanos);
    u64::try_from(delta).unwrap_or(u64::MAX)
}

/// Estimates the local-clock offset relative to the backend, in nanoseconds.
///
/// Following the gateway sampling algorithm, the backend's true time at our
/// receive instant is approximated as `server_send + rtt / 2` (half the
/// round-trip accounts for the return path), and the offset is that estimate
/// minus our local wall-clock receive time. A positive offset means the local
/// clock is behind the backend.
#[must_use]
pub fn estimate_offset_nanos(
    server_send_unix_nanos: i128,
    local_receive_unix_nanos: i128,
    rtt_nanos: u64,
) -> i128 {
    let half_rtt = i128::from(rtt_nanos / 2);
    server_send_unix_nanos + half_rtt - local_receive_unix_nanos
}

/// Returns the median of a set of offsets in nanoseconds, or `None` if empty.
///
/// For an even count the mean of the two central values is returned; the
/// intermediate sum uses `i128` so it cannot overflow for realistic offsets.
#[must_use]
pub fn median_offset_nanos(offsets: &[i128]) -> Option<i128> {
    if offsets.is_empty() {
        return None;
    }
    let mut sorted = offsets.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[mid])
    } else {
        Some((sorted[mid - 1] + sorted[mid]) / 2)
    }
}

/// Returns whether an offset deviates from the median by more than the threshold.
///
/// `threshold_nanos` is the maximum allowed absolute deviation; samples beyond
/// it are treated as outliers and discarded.
#[must_use]
pub fn is_outlier(offset_nanos: i128, median_nanos: i128, threshold_nanos: u64) -> bool {
    let deviation = (offset_nanos - median_nanos).unsigned_abs();
    deviation > u128::from(threshold_nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtt_is_difference_of_monotonic_readings() {
        assert_eq!(rtt_nanos(1_000, 1_500), 500);
    }

    #[test]
    fn rtt_saturates_when_clock_goes_backwards() {
        assert_eq!(rtt_nanos(1_500, 1_000), 0);
    }

    #[test]
    fn offset_adds_half_rtt_and_subtracts_local_time() {
        // server_send = 1_000, rtt = 400 -> +200; local_receive = 900 -> offset 300.
        assert_eq!(estimate_offset_nanos(1_000, 900, 400), 300);
    }

    #[test]
    fn offset_can_be_negative_when_local_clock_is_ahead() {
        assert_eq!(estimate_offset_nanos(1_000, 1_500, 0), -500);
    }

    #[test]
    fn median_of_odd_count_is_central_value() {
        assert_eq!(median_offset_nanos(&[30, 10, 20]), Some(20));
    }

    #[test]
    fn median_of_even_count_is_mean_of_central_pair() {
        assert_eq!(median_offset_nanos(&[10, 20, 30, 40]), Some(25));
    }

    #[test]
    fn median_of_empty_is_none() {
        assert_eq!(median_offset_nanos(&[]), None);
    }

    #[test]
    fn outlier_detection_uses_absolute_deviation() {
        assert!(is_outlier(250, 0, 100));
        assert!(is_outlier(-250, 0, 100));
        assert!(!is_outlier(50, 0, 100));
        assert!(!is_outlier(100, 0, 100));
    }
}
