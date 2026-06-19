//! Per-round sample filtering: latency ceiling, outlier rejection, selection.
//!
//! The filter operates on the good samples a round collected and decides which
//! survive and which single sample best represents the round.

use chronos_core::estimate::{is_outlier, median_offset_nanos};
use chronos_core::TimeSample;

/// Thresholds applied to a round of good samples.
#[derive(Debug, Clone, Copy)]
pub struct RoundFilter {
    /// Round-trip-time ceiling in nanoseconds; samples above it are rejected.
    pub max_rtt_nanos: u64,
    /// Maximum absolute deviation in nanoseconds from the round median offset.
    pub outlier_threshold_nanos: u64,
}

/// Result of filtering a round.
#[derive(Debug, Clone)]
pub struct FilterOutcome {
    /// Samples that survived both the latency and outlier filters.
    pub good_samples: Vec<TimeSample>,
    /// Count of samples rejected for exceeding the latency ceiling.
    pub high_latency: usize,
    /// Count of samples rejected as offset outliers.
    pub outliers: usize,
    /// The median-offset sample among the survivors, if any.
    pub selected: Option<TimeSample>,
}

impl RoundFilter {
    /// Applies the latency and outlier filters and selects a representative.
    ///
    /// High-RTT samples are removed first; the median offset is then computed
    /// over the survivors and samples deviating beyond the threshold are
    /// dropped. The selected sample is the median-offset survivor.
    #[must_use]
    pub fn apply(&self, samples: Vec<TimeSample>) -> FilterOutcome {
        let mut within_rtt = Vec::with_capacity(samples.len());
        let mut high_latency = 0usize;
        for sample in samples {
            if sample.rtt_nanos > self.max_rtt_nanos {
                high_latency += 1;
            } else {
                within_rtt.push(sample);
            }
        }

        let offsets: Vec<i128> = within_rtt
            .iter()
            .map(|sample| sample.estimated_offset_nanos)
            .collect();

        let (good_samples, outliers) = match median_offset_nanos(&offsets) {
            Some(median) => {
                let mut kept = Vec::with_capacity(within_rtt.len());
                let mut outliers = 0usize;
                for sample in within_rtt {
                    if is_outlier(
                        sample.estimated_offset_nanos,
                        median,
                        self.outlier_threshold_nanos,
                    ) {
                        outliers += 1;
                    } else {
                        kept.push(sample);
                    }
                }
                (kept, outliers)
            }
            None => (within_rtt, 0),
        };

        let selected = select_median_sample(&good_samples);
        FilterOutcome {
            good_samples,
            high_latency,
            outliers,
            selected,
        }
    }
}

/// Returns the median-offset sample, cloning the central element after sorting.
fn select_median_sample(samples: &[TimeSample]) -> Option<TimeSample> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted: Vec<&TimeSample> = samples.iter().collect();
    sorted.sort_by_key(|sample| sample.estimated_offset_nanos);
    Some(sorted[sorted.len() / 2].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_core::SampleQuality;

    fn sample(rtt_nanos: u64, offset_nanos: i128) -> TimeSample {
        TimeSample {
            backend_name: "lab".to_string(),
            server_send_unix_nanos: 0,
            local_receive_unix_nanos: 0,
            rtt_nanos,
            estimated_offset_nanos: offset_nanos,
            quality: SampleQuality::Good,
        }
    }

    #[test]
    fn rejects_high_latency_samples() {
        let filter = RoundFilter {
            max_rtt_nanos: 1_000,
            outlier_threshold_nanos: 1_000_000,
        };
        let outcome = filter.apply(vec![sample(500, 10), sample(2_000, 10), sample(800, 10)]);
        assert_eq!(outcome.high_latency, 1);
        assert_eq!(outcome.good_samples.len(), 2);
    }

    #[test]
    fn rejects_offset_outliers() {
        let filter = RoundFilter {
            max_rtt_nanos: 10_000,
            outlier_threshold_nanos: 100,
        };
        let outcome = filter.apply(vec![
            sample(100, 10),
            sample(100, 20),
            sample(100, 30),
            sample(100, 5_000),
        ]);
        assert_eq!(outcome.outliers, 1);
        assert_eq!(outcome.good_samples.len(), 3);
    }

    #[test]
    fn selects_median_offset_sample() {
        let filter = RoundFilter {
            max_rtt_nanos: 10_000,
            outlier_threshold_nanos: 1_000_000,
        };
        let outcome = filter.apply(vec![sample(100, 30), sample(100, 10), sample(100, 20)]);
        assert_eq!(
            outcome.selected.expect("selected").estimated_offset_nanos,
            20
        );
    }

    #[test]
    fn empty_round_selects_nothing() {
        let filter = RoundFilter {
            max_rtt_nanos: 10_000,
            outlier_threshold_nanos: 1_000_000,
        };
        let outcome = filter.apply(vec![]);
        assert!(outcome.selected.is_none());
        assert!(outcome.good_samples.is_empty());
    }
}
