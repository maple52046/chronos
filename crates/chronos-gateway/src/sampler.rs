//! Burst sampling: collect a round of samples and reduce it to one good sample.

use chronos_core::{GatewayState, SampleQuality, TimeSample};

use crate::backend_client::BackendClient;
use crate::filter::RoundFilter;

/// Collects and filters bursts of samples from a backend.
#[derive(Debug, Clone)]
pub struct Sampler {
    burst_samples: u32,
    min_good_samples: usize,
    filter: RoundFilter,
}

/// Outcome of one sampling round against a single backend.
#[derive(Debug, Clone)]
pub struct RoundResult {
    /// The selected median sample, present only when enough good samples passed.
    pub selected: Option<TimeSample>,
    /// Number of samples that survived filtering.
    pub good_samples: usize,
    /// Total requests issued in the round.
    pub attempts: usize,
    /// Samples rejected for exceeding the latency ceiling.
    pub high_latency: usize,
    /// Samples rejected as offset outliers.
    pub outliers: usize,
    /// Qualities of samples rejected at request/parse time.
    pub request_failures: Vec<SampleQuality>,
}

impl Sampler {
    /// Creates a sampler from burst size, the good-sample minimum, and a filter.
    #[must_use]
    pub fn new(burst_samples: u32, min_good_samples: u32, filter: RoundFilter) -> Self {
        Self {
            burst_samples,
            min_good_samples: min_good_samples as usize,
            filter,
        }
    }

    /// Runs one burst round against `client` and reduces it to a result.
    ///
    /// A selected sample is returned only when at least `min_good_samples`
    /// survive filtering, so a partially failed round never feeds the backend.
    pub async fn sample_backend(&self, client: &BackendClient) -> RoundResult {
        let mut good = Vec::new();
        let mut request_failures = Vec::new();
        for _ in 0..self.burst_samples {
            match client.fetch_sample().await {
                Ok(sample) => good.push(sample),
                Err(quality) => request_failures.push(quality),
            }
        }

        let attempts = self.burst_samples as usize;
        let outcome = self.filter.apply(good);
        let good_samples = outcome.good_samples.len();
        let selected = if good_samples >= self.min_good_samples {
            outcome.selected
        } else {
            None
        };

        RoundResult {
            selected,
            good_samples,
            attempts,
            high_latency: outcome.high_latency,
            outliers: outcome.outliers,
            request_failures,
        }
    }
}

/// Computes the next gateway state from the previous one and a round result.
///
/// A successful round is always [`GatewayState::Synchronized`]. A failure
/// degrades a previously good state to [`GatewayState::Degraded`] (the last good
/// sample may still be serving), while a gateway that has never synchronized
/// reports [`GatewayState::Unsynchronized`].
#[must_use]
pub fn next_state(previous: GatewayState, round_succeeded: bool) -> GatewayState {
    if round_succeeded {
        GatewayState::Synchronized
    } else {
        match previous {
            GatewayState::Synchronized | GatewayState::Degraded => GatewayState::Degraded,
            _ => GatewayState::Unsynchronized,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendConfig;
    use chronos_core::SecurityPolicy;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn synced_body() -> serde_json::Value {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after epoch")
            .as_nanos() as i128;
        serde_json::json!({
            "version": 1,
            "unix_sec": (now / 1_000_000_000) as i64,
            "unix_nano": now,
            "server_recv_unix_nano": now,
            "server_send_unix_nano": now,
            "status": {
                "provider": "chrony",
                "sync": "synchronized",
                "stratum": 3,
                "last_offset_nanos": 0
            }
        })
    }

    fn lab_client(uri: String) -> BackendClient {
        let config = BackendConfig {
            name: "lab".to_string(),
            url: format!("{uri}/time"),
            require_tls: false,
            require_valid_cert: false,
        };
        BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[],
        )
        .expect("client")
    }

    fn sampler() -> Sampler {
        Sampler::new(
            5,
            3,
            RoundFilter {
                max_rtt_nanos: 5_000_000_000,
                outlier_threshold_nanos: 1_000_000_000,
            },
        )
    }

    #[tokio::test]
    async fn round_selects_sample_when_enough_good() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(200).set_body_json(synced_body()))
            .mount(&server)
            .await;

        let result = sampler().sample_backend(&lab_client(server.uri())).await;
        assert!(result.selected.is_some());
        assert!(result.good_samples >= 3);
    }

    #[tokio::test]
    async fn round_fails_when_all_requests_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let result = sampler().sample_backend(&lab_client(server.uri())).await;
        assert!(result.selected.is_none());
        assert_eq!(result.good_samples, 0);
        assert_eq!(result.request_failures.len(), 5);
    }

    #[test]
    fn state_transitions_follow_round_outcome() {
        assert_eq!(
            next_state(GatewayState::Starting, true),
            GatewayState::Synchronized
        );
        assert_eq!(
            next_state(GatewayState::Starting, false),
            GatewayState::Unsynchronized
        );
        assert_eq!(
            next_state(GatewayState::Synchronized, false),
            GatewayState::Degraded
        );
        assert_eq!(
            next_state(GatewayState::Degraded, false),
            GatewayState::Degraded
        );
        assert_eq!(
            next_state(GatewayState::Degraded, true),
            GatewayState::Synchronized
        );
    }
}
