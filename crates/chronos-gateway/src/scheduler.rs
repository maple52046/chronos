//! Periodic sampling loop, chrony writing, and gateway state tracking.

use std::sync::Arc;
use std::time::Duration;

use chronos_core::{GatewayState, OutputBackend};

use crate::backend_client::BackendClient;
use crate::sampler::{next_state, Sampler};
use crate::status_api::SharedStatus;

/// Drives sampling rounds on a fixed interval, writes good samples to the
/// output backend, and tracks the gateway state.
pub struct Scheduler {
    clients: Vec<BackendClient>,
    sampler: Sampler,
    interval: Duration,
    output: Arc<dyn OutputBackend + Send + Sync>,
    status: SharedStatus,
}

impl Scheduler {
    /// Creates a scheduler over backends, sampler, interval, output, and status.
    #[must_use]
    pub fn new(
        clients: Vec<BackendClient>,
        sampler: Sampler,
        interval: Duration,
        output: Arc<dyn OutputBackend + Send + Sync>,
        status: SharedStatus,
    ) -> Self {
        Self {
            clients,
            sampler,
            interval,
            output,
            status,
        }
    }

    /// Runs sampling rounds forever, sleeping `interval` between rounds.
    ///
    /// # Errors
    ///
    /// Currently never returns; the signature keeps `?`-propagation available
    /// for graceful-shutdown wiring added in later milestones.
    pub async fn run(self) -> anyhow::Result<()> {
        let mut state = GatewayState::Starting;
        let mut consecutive_failures: u32 = 0;
        // Recover quickly after a transient outage without hammering a down
        // backend: retry sooner than the steady interval, easing back up to it.
        let base = Duration::from_secs(1).min(self.interval);
        loop {
            let succeeded = self.run_round().await;
            state = next_state(state, succeeded);
            self.status.update_state(state);
            let delay = if succeeded {
                consecutive_failures = 0;
                self.interval
            } else {
                consecutive_failures = consecutive_failures.saturating_add(1);
                backoff_delay(consecutive_failures, base, self.interval)
            };
            tracing::info!(
                state = ?state,
                consecutive_failures,
                delay_ms = delay.as_millis() as u64,
                "sampling round complete"
            );
            tokio::time::sleep(delay).await;
        }
    }

    /// Samples backends in priority order, writing the first usable result to
    /// the output backend.
    ///
    /// A failed chrony write does not promote the round to a failure: the
    /// backend sample is still good, so the gateway stays synchronized while the
    /// write error is surfaced on the status endpoint.
    async fn run_round(&self) -> bool {
        for client in &self.clients {
            let result = self.sampler.sample_backend(client).await;
            let Some(sample) = result.selected else {
                tracing::warn!(
                    backend = client.name(),
                    attempts = result.attempts,
                    good = result.good_samples,
                    high_latency = result.high_latency,
                    outliers = result.outliers,
                    failures = result.request_failures.len(),
                    "round produced no usable sample"
                );
                continue;
            };

            let write_result = self.output.submit_sample(&sample);
            match &write_result {
                Ok(()) => tracing::info!(
                    backend = client.name(),
                    offset_nanos = sample.estimated_offset_nanos as i64,
                    rtt_nanos = sample.rtt_nanos,
                    good = result.good_samples,
                    "wrote sample to chrony"
                ),
                Err(err) => tracing::error!(
                    backend = client.name(),
                    error = %err,
                    "failed to write sample to chrony"
                ),
            }
            self.status.record_sample(
                client.name(),
                client.url().as_str(),
                &sample,
                write_result.map_err(|err| err.to_string()),
            );
            return true;
        }
        false
    }
}

/// Computes a capped exponential backoff delay for a failed round.
///
/// The delay is `base * 2^(attempt - 1)` for `attempt >= 1`, clamped to `max`
/// (the steady sampling interval). The first retry waits `base`, and the delay
/// doubles each consecutive failure until it reaches the interval.
fn backoff_delay(attempt: u32, base: Duration, max: Duration) -> Duration {
    if attempt <= 1 {
        return base.min(max);
    }
    let factor = u32::try_from(2u64.saturating_pow(attempt - 1)).unwrap_or(u32::MAX);
    base.saturating_mul(factor).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use chronos_core::{ChronosError, TimeSample};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::BackendConfig;
    use crate::filter::RoundFilter;
    use chronos_core::SecurityPolicy;

    struct RecordingBackend {
        fail: bool,
        writes: AtomicUsize,
        last_backend: Mutex<Option<String>>,
    }

    impl RecordingBackend {
        fn new(fail: bool) -> Self {
            Self {
                fail,
                writes: AtomicUsize::new(0),
                last_backend: Mutex::new(None),
            }
        }
    }

    impl OutputBackend for RecordingBackend {
        fn submit_sample(&self, sample: &TimeSample) -> Result<(), ChronosError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            *self.last_backend.lock().expect("lock") = Some(sample.backend_name.clone());
            if self.fail {
                Err(ChronosError::OutputBackend(
                    "socket unavailable".to_string(),
                ))
            } else {
                Ok(())
            }
        }
    }

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

    fn client(name: &str, uri: String) -> BackendClient {
        let config = BackendConfig {
            name: name.to_string(),
            base_url: uri,
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
            3,
            2,
            RoundFilter {
                max_rtt_nanos: 5_000_000_000,
                outlier_threshold_nanos: 1_000_000_000,
            },
        )
    }

    #[test]
    fn backoff_grows_and_caps_at_interval() {
        let base = Duration::from_secs(1);
        let max = Duration::from_secs(30);
        assert_eq!(backoff_delay(1, base, max), Duration::from_secs(1));
        assert_eq!(backoff_delay(2, base, max), Duration::from_secs(2));
        assert_eq!(backoff_delay(3, base, max), Duration::from_secs(4));
        assert_eq!(backoff_delay(10, base, max), max);
    }

    #[tokio::test]
    async fn failover_uses_second_backend_when_first_fails() {
        let bad = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&bad)
            .await;
        let good = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(200).set_body_json(synced_body()))
            .mount(&good)
            .await;

        let output = Arc::new(RecordingBackend::new(false));
        let scheduler = Scheduler::new(
            vec![client("bad", bad.uri()), client("good", good.uri())],
            sampler(),
            Duration::from_secs(30),
            output.clone(),
            SharedStatus::new("/run/chrony/chronos.sock"),
        );

        assert!(scheduler.run_round().await);
        assert_eq!(output.writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            output.last_backend.lock().expect("lock").as_deref(),
            Some("good")
        );
    }

    #[tokio::test]
    async fn all_backends_unreachable_fails_round() {
        let down = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&down)
            .await;

        let output = Arc::new(RecordingBackend::new(false));
        let scheduler = Scheduler::new(
            vec![client("down", down.uri())],
            sampler(),
            Duration::from_secs(30),
            output.clone(),
            SharedStatus::new("/run/chrony/chronos.sock"),
        );

        assert!(!scheduler.run_round().await);
        assert_eq!(output.writes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn chrony_write_failure_keeps_round_successful() {
        let good = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(200).set_body_json(synced_body()))
            .mount(&good)
            .await;

        let output = Arc::new(RecordingBackend::new(true));
        let scheduler = Scheduler::new(
            vec![client("good", good.uri())],
            sampler(),
            Duration::from_secs(30),
            output.clone(),
            SharedStatus::new("/run/chrony/chronos.sock"),
        );

        // A failed chrony write does not invalidate a good backend sample.
        assert!(scheduler.run_round().await);
        assert_eq!(output.writes.load(Ordering::SeqCst), 1);
    }
}
