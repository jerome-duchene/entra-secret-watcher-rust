use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use subtle::ConstantTimeEq;

use crate::metrics::ScanMetrics;

pub const TOKEN_HEADER: &str = "X-Scan-Token";

type StartScan = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
struct AppState {
    tenant_name: String,
    metrics: Arc<ScanMetrics>,
}

#[derive(Clone)]
struct ScanState {
    secret: String,
    start_scan: StartScan,
}

/// `GET /health`, `GET /metrics`, and `POST /scan` which is only mapped when a secret is configured.
pub fn router(
    tenant_name: String,
    metrics: Arc<ScanMetrics>,
    manual_trigger_secret: String,
    start_scan: StartScan,
) -> Router {
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(prometheus))
        .with_state(AppState { tenant_name, metrics });

    if !manual_trigger_secret.is_empty() {
        router = router.merge(
            Router::new()
                .route("/scan", post(trigger_scan))
                .with_state(ScanState { secret: manual_trigger_secret, start_scan }),
        );
    }
    router
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "tenant": state.tenant_name,
        "timestamp": chrono::Utc::now(),
    }))
}

async fn prometheus(State(state): State<AppState>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], state.metrics.render_prometheus())
}

async fn trigger_scan(State(state): State<ScanState>, headers: HeaderMap) -> StatusCode {
    let provided = headers.get(TOKEN_HEADER).map(|v| v.as_bytes());
    if !is_authorized(provided, &state.secret) {
        return StatusCode::UNAUTHORIZED;
    }

    (state.start_scan)();
    StatusCode::ACCEPTED
}

fn is_authorized(provided: Option<&[u8]>, expected: &str) -> bool {
    match provided {
        Some(provided) if !provided.is_empty() && !expected.is_empty() => provided.ct_eq(expected.as_bytes()).into(),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
