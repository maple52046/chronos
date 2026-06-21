//! Local status HTTP endpoint (`/healthz`, `/status`) and shared state.
//!
//! The scheduler updates [`SharedStatus`] after each round; the HTTP handlers
//! read a snapshot. State is shared through an `Arc<Mutex<_>>`; locks are held
//! only for the duration of a snapshot copy and never across an `.await`.

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chronos_core::{GatewayState, SampleQuality, TimeSample};
use serde::Serialize;

/// Thread-safe handle to the gateway's live status.
#[derive(Clone)]
pub struct SharedStatus {
    inner: Arc<Mutex<StatusState>>,
}

#[derive(Debug, Clone)]
struct StatusState {
    state: GatewayState,
    backend_name: Option<String>,
    backend_url: Option<String>,
    last_success_unix_sec: Option<i64>,
    last_rtt_ms: Option<u64>,
    last_offset_ms: Option<i64>,
    last_quality: Option<SampleQuality>,
    output_kind: String,
    output_target: String,
    last_write: String,
}

impl SharedStatus {
    /// Creates a status handle in the [`GatewayState::Starting`] state.
    ///
    /// `kind` is the output backend discriminant (e.g. `chrony_sock`,
    /// `ntp_shm`) and `target` is its operator-facing description.
    #[must_use]
    pub fn new(kind: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StatusState {
                state: GatewayState::Starting,
                backend_name: None,
                backend_url: None,
                last_success_unix_sec: None,
                last_rtt_ms: None,
                last_offset_ms: None,
                last_quality: None,
                output_kind: kind.into(),
                output_target: target.into(),
                last_write: "pending".to_string(),
            })),
        }
    }

    /// Updates the reported lifecycle state.
    pub fn update_state(&self, state: GatewayState) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.state = state;
        }
    }

    /// Records a selected sample and the result of writing it to the output
    /// backend.
    pub fn record_sample(
        &self,
        backend_name: &str,
        backend_url: &str,
        sample: &TimeSample,
        write_result: Result<(), String>,
    ) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.backend_name = Some(backend_name.to_string());
            guard.backend_url = Some(backend_url.to_string());
            guard.last_success_unix_sec =
                Some((sample.local_receive_unix_nanos / 1_000_000_000) as i64);
            guard.last_rtt_ms = Some(sample.rtt_nanos / 1_000_000);
            guard.last_offset_ms = Some((sample.estimated_offset_nanos / 1_000_000) as i64);
            guard.last_quality = Some(sample.quality);
            guard.last_write = match write_result {
                Ok(()) => "ok".to_string(),
                Err(detail) => format!("error: {detail}"),
            };
        }
    }
}

/// Builds the status router with `/healthz` and `/status` routes.
pub fn router(status: SharedStatus) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/status", get(status_handler))
        .with_state(status)
}

#[derive(Debug, Serialize)]
struct BackendInfo {
    name: String,
    url: String,
    last_success_unix_sec: Option<i64>,
}

#[derive(Debug, Serialize)]
struct LastSample {
    rtt_ms: Option<u64>,
    estimated_offset_ms: Option<i64>,
    quality: Option<SampleQuality>,
}

#[derive(Debug, Serialize)]
struct OutputInfo {
    kind: String,
    target: String,
    last_write: String,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    service: &'static str,
    state: GatewayState,
    backend: Option<BackendInfo>,
    last_sample: LastSample,
    output: OutputInfo,
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn status_handler(State(status): State<SharedStatus>) -> Json<StatusResponse> {
    let snapshot = match status.inner.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let backend = snapshot.backend_name.clone().map(|name| BackendInfo {
        name,
        url: snapshot.backend_url.clone().unwrap_or_default(),
        last_success_unix_sec: snapshot.last_success_unix_sec,
    });
    Json(StatusResponse {
        service: "chronos-gateway",
        state: snapshot.state,
        backend,
        last_sample: LastSample {
            rtt_ms: snapshot.last_rtt_ms,
            estimated_offset_ms: snapshot.last_offset_ms,
            quality: snapshot.last_quality,
        },
        output: OutputInfo {
            kind: snapshot.output_kind,
            target: snapshot.output_target,
            last_write: snapshot.last_write,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn sample() -> TimeSample {
        TimeSample {
            backend_name: "primary".to_string(),
            server_send_unix_nanos: 0,
            local_receive_unix_nanos: 1_781_844_000_000_000_000,
            rtt_nanos: 42_000_000,
            estimated_offset_nanos: 3_000_000,
            quality: SampleQuality::Good,
        }
    }

    async fn spawn(status: SharedStatus) -> SocketAddr {
        crate::ensure_crypto_provider();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, router(status)).await.expect("serve");
        });
        addr
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let addr = spawn(SharedStatus::new(
            "chrony_sock",
            "chrony SOCK /run/chrony/chronos.sock",
        ))
        .await;
        let body: serde_json::Value = reqwest::get(format!("http://{addr}/healthz"))
            .await
            .expect("request")
            .json()
            .await
            .expect("json");
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn status_reflects_recorded_sample() {
        let status = SharedStatus::new("ntp_shm", "ntp shm unit 2 (127.127.28.2)");
        status.update_state(GatewayState::Synchronized);
        status.record_sample(
            "primary",
            "https://time.example.com/time",
            &sample(),
            Ok(()),
        );
        let addr = spawn(status).await;
        let body: serde_json::Value = reqwest::get(format!("http://{addr}/status"))
            .await
            .expect("request")
            .json()
            .await
            .expect("json");
        assert_eq!(body["service"], "chronos-gateway");
        assert_eq!(body["state"], "synchronized");
        assert_eq!(body["backend"]["name"], "primary");
        assert_eq!(body["last_sample"]["rtt_ms"], 42);
        assert_eq!(body["last_sample"]["estimated_offset_ms"], 3);
        assert_eq!(body["last_sample"]["quality"], "Good");
        assert_eq!(body["output"]["kind"], "ntp_shm");
        assert_eq!(body["output"]["target"], "ntp shm unit 2 (127.127.28.2)");
        assert_eq!(body["output"]["last_write"], "ok");
    }
}
