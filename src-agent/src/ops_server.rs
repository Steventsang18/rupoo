//! Minimal ops server for the serve daemon.
//!
//! Exposes two endpoints, bound to loopback by default:
//!
//! - `GET /healthz` — liveness probe (status, version, uptime, pid)
//! - `GET /metrics` — Prometheus text snapshot from [`crate::telemetry`]
//!
//! The server is intentionally small: it is the skeleton of the future
//! admin API surface, and carries a concurrency limit so a misbehaving
//! consumer cannot tie up the runtime. A bind failure is **not** fatal —
//! the daemon logs a warning and keeps running without it.

use std::net::SocketAddr;
use std::time::Instant;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use once_cell::sync::Lazy;
use tower::limit::ConcurrencyLimitLayer;
use tracing::{info, warn};

/// Process start instant, used for uptime reporting.
static STARTED: Lazy<Instant> = Lazy::new(Instant::now);

/// Configuration for the ops server, mirroring the `[server]` config section.
#[derive(Debug, Clone)]
pub struct OpsServerConfig {
    /// `host:port` bind address.
    pub listen: String,
    /// Maximum concurrent requests accepted by the server.
    pub max_concurrency: usize,
}

/// Spawn the ops server on a background task.
///
/// Returns `None` (with a warning logged) when the address is invalid or
/// the port is already taken — the serve daemon must keep running either way.
pub async fn spawn_ops_server(cfg: OpsServerConfig) -> Option<tokio::task::JoinHandle<()>> {
    let addr: SocketAddr = match cfg.listen.parse() {
        Ok(addr) => addr,
        Err(e) => {
            warn!(listen = %cfg.listen, error = %e, "ops server: invalid listen address, disabled");
            return None;
        }
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .layer(ConcurrencyLimitLayer::new(cfg.max_concurrency.max(1)));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            warn!(listen = %addr, error = %e, "ops server: bind failed, disabled");
            return None;
        }
    };

    info!(listen = %addr, "ops server listening");
    Some(tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            warn!(error = %e, "ops server stopped unexpectedly");
        }
    }))
}

/// `GET /healthz` — liveness probe for process supervisors.
async fn healthz() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": STARTED.elapsed().as_secs(),
        "pid": std::process::id(),
    }))
}

/// `GET /metrics` — Prometheus text snapshot.
///
/// 503 when the recorder was not installed (recorder init failed at boot).
async fn metrics() -> impl IntoResponse {
    match crate::telemetry::render_prometheus() {
        Some(body) => (StatusCode::OK, body).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "metrics recorder unavailable",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    fn test_router() -> Router {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/metrics", get(metrics))
            .layer(ConcurrencyLimitLayer::new(8))
    }

    #[tokio::test]
    async fn test_healthz_returns_ok() {
        let app = test_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn test_metrics_returns_prometheus_text() {
        assert!(crate::telemetry::init());
        crate::telemetry::record_message_received("feishu");
        let app = test_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("rupoo_messages_received_total"));
    }

    #[tokio::test]
    async fn test_unknown_route_404s() {
        let app = test_router();
        let resp = app
            .oneshot(Request::builder().uri("/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
