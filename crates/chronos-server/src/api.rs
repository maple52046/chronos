//! HTTP Time API: router, request handlers, and response shapes.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chronos_core::{SyncState, TimeProvider, TimeStatus, TimeStatusProvider};
use serde::Serialize;

/// Shared handler state.
#[derive(Clone)]
pub struct AppState {
    provider: Arc<dyn TimeStatusProvider + Send + Sync>,
    cache_control: Arc<str>,
    allow_unknown_status: bool,
}

impl AppState {
    /// Builds handler state from a status provider and API configuration.
    #[must_use]
    pub fn new(
        provider: Arc<dyn TimeStatusProvider + Send + Sync>,
        cache_control: impl Into<Arc<str>>,
        allow_unknown_status: bool,
    ) -> Self {
        Self {
            provider,
            cache_control: cache_control.into(),
            allow_unknown_status,
        }
    }
}

/// Builds the Time API router with `/time`, `/healthz`, and `/status` routes,
/// optionally mounted under `base_path` (e.g. `/chronos`).
///
/// An empty `base_path` serves the routes at the root; a non-empty prefix is
/// expected in canonical form (leading slash, no trailing slash) as produced by
/// [`crate::config::ApiConfig`].
pub fn router(state: AppState, base_path: &str) -> Router {
    let routes = Router::new()
        .route("/time", get(time))
        .route("/healthz", get(healthz))
        .route("/status", get(status))
        .with_state(state);
    if base_path.is_empty() {
        routes
    } else {
        Router::new().nest(base_path, routes)
    }
}

/// `GET /time` response body, API version 1.
#[derive(Debug, Serialize)]
struct TimeResponse {
    version: u32,
    unix_sec: i64,
    unix_nano: i128,
    server_recv_unix_nano: i128,
    server_send_unix_nano: i128,
    status: TimeStatus,
}

/// `GET /status` response body.
#[derive(Debug, Serialize)]
struct ServerStatus {
    service: &'static str,
    state: &'static str,
    time_status: TimeStatus,
}

async fn time(State(state): State<AppState>) -> Response {
    let server_recv = now_unix_nanos();
    let status = read_time_status(&state).await;
    let server_send = now_unix_nanos();
    let body = TimeResponse {
        version: 1,
        unix_sec: (server_send / 1_000_000_000) as i64,
        unix_nano: server_send,
        server_recv_unix_nano: server_recv,
        server_send_unix_nano: server_send,
        status,
    };
    json_no_store(&state.cache_control, &body)
}

async fn healthz(State(state): State<AppState>) -> Response {
    json_no_store(&state.cache_control, &serde_json::json!({ "status": "ok" }))
}

async fn status(State(state): State<AppState>) -> Response {
    let time_status = read_time_status(&state).await;
    // `allow_unknown_status` decides whether an undeterminable sync state is
    // reported as healthy `running` or as `degraded`; the process itself is
    // always live, so `/healthz` is unaffected.
    let state_label =
        if matches!(time_status.sync, SyncState::Unknown) && !state.allow_unknown_status {
            "degraded"
        } else {
            "running"
        };
    let body = ServerStatus {
        service: "chronos-server",
        state: state_label,
        time_status,
    };
    json_no_store(&state.cache_control, &body)
}

/// Reads the current time status, degrading provider failures to an unknown
/// status so the time endpoint keeps serving.
async fn read_time_status(state: &AppState) -> TimeStatus {
    let provider_id = state.provider.provider();
    let provider = state.provider.clone();
    match tokio::task::spawn_blocking(move || provider.time_status()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "time status unavailable; reporting unknown");
            unknown_status(provider_id)
        }
        Err(err) => {
            tracing::error!(error = %err, "status task failed; reporting unknown");
            unknown_status(provider_id)
        }
    }
}

fn unknown_status(provider: TimeProvider) -> TimeStatus {
    TimeStatus {
        provider,
        sync: SyncState::Unknown,
        stratum: None,
        last_offset_nanos: None,
    }
}

/// Serializes `body` as JSON and applies the configured `Cache-Control` header.
fn json_no_store<T: Serialize>(cache_control: &str, body: &T) -> Response {
    let mut response = Json(body).into_response();
    if let Ok(value) = HeaderValue::from_str(cache_control) {
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }
    response
}

/// Returns the current wall-clock time as Unix nanoseconds.
///
/// Times before the Unix epoch yield a negative value rather than panicking.
fn now_unix_nanos() -> i128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_nanos() as i128,
        Err(err) => -(err.duration().as_nanos() as i128),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    use chronos_core::{BackendStatus, ChronosError};

    struct FakeProvider(BackendStatus);

    impl TimeStatusProvider for FakeProvider {
        fn provider(&self) -> TimeProvider {
            TimeProvider::Chrony
        }

        fn backend_status(&self) -> Result<BackendStatus, ChronosError> {
            Ok(self.0.clone())
        }
    }

    async fn spawn(state: AppState) -> SocketAddr {
        spawn_with_prefix(state, "").await
    }

    async fn spawn_with_prefix(state: AppState, base_path: &str) -> SocketAddr {
        crate::ensure_crypto_provider();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let app = router(state, base_path);
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        addr
    }

    fn synced_state() -> AppState {
        let provider = Arc::new(FakeProvider(BackendStatus::Synchronized {
            stratum: Some(3),
            last_offset_nanos: Some(12_000),
        }));
        AppState::new(provider, "no-store", false)
    }

    #[tokio::test]
    async fn time_endpoint_returns_v1_json_with_no_store() {
        let addr = spawn(synced_state()).await;
        let response = reqwest::get(format!("http://{addr}/time"))
            .await
            .expect("request");
        assert_eq!(
            response.headers().get("cache-control").expect("header"),
            "no-store"
        );
        let body: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(body["version"], 1);
        assert_eq!(body["status"]["provider"], "chrony");
        assert_eq!(body["status"]["sync"], "synchronized");
        assert_eq!(body["status"]["stratum"], 3);
        assert_eq!(body["status"]["last_offset_nanos"], 12_000);
        let recv = body["server_recv_unix_nano"].as_i64().expect("recv");
        let send = body["server_send_unix_nano"].as_i64().expect("send");
        assert!(recv <= send);
        assert_eq!(body["unix_nano"].as_i64().expect("unix_nano"), send);
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let addr = spawn(synced_state()).await;
        let body: serde_json::Value = reqwest::get(format!("http://{addr}/healthz"))
            .await
            .expect("request")
            .json()
            .await
            .expect("json body");
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn status_reports_running_when_synchronized() {
        let addr = spawn(synced_state()).await;
        let body: serde_json::Value = reqwest::get(format!("http://{addr}/status"))
            .await
            .expect("request")
            .json()
            .await
            .expect("json body");
        assert_eq!(body["service"], "chronos-server");
        assert_eq!(body["state"], "running");
        assert_eq!(body["time_status"]["sync"], "synchronized");
    }

    #[tokio::test]
    async fn routes_are_served_under_base_path() {
        let addr = spawn_with_prefix(synced_state(), "/chronos").await;
        let prefixed: serde_json::Value = reqwest::get(format!("http://{addr}/chronos/time"))
            .await
            .expect("request")
            .json()
            .await
            .expect("json body");
        assert_eq!(prefixed["version"], 1);
        let unprefixed = reqwest::get(format!("http://{addr}/time"))
            .await
            .expect("request");
        assert_eq!(unprefixed.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn status_reports_degraded_when_unknown_disallowed() {
        let provider = Arc::new(FakeProvider(BackendStatus::Unknown));
        let addr = spawn(AppState::new(provider, "no-store", false)).await;
        let body: serde_json::Value = reqwest::get(format!("http://{addr}/status"))
            .await
            .expect("request")
            .json()
            .await
            .expect("json body");
        assert_eq!(body["state"], "degraded");
    }
}
