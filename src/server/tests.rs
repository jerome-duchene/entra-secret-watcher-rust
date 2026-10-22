use super::*;
use crate::models::test_support::{credential, scan};
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

const SECRET: &str = "a-long-random-secret";

fn app(secret: &str) -> (Router, Arc<AtomicUsize>, Arc<ScanMetrics>) {
    let started = Arc::new(AtomicUsize::new(0));
    let counter = started.clone();
    let metrics = Arc::new(ScanMetrics::new("Contoso", "tenant-id"));
    let router = router(
        "Contoso".into(),
        metrics.clone(),
        secret.into(),
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    (router, started, metrics)
}

async fn call(router: Router, request: Request<Body>) -> (StatusCode, String) {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

fn post_scan(token: Option<&str>) -> Request<Body> {
    let mut builder = Request::post("/scan");
    if let Some(token) = token {
        builder = builder.header(TOKEN_HEADER, token);
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn health_reports_the_tenant() {
    let (router, _, _) = app("");
    let (status, body) = call(router, Request::get("/health").body(Body::empty()).unwrap()).await;

    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "healthy");
    assert_eq!(json["tenant"], "Contoso");
    assert!(json["timestamp"].is_string());
}

#[tokio::test]
async fn metrics_are_served_in_prometheus_format() {
    let (router, _, metrics) = app("");
    metrics.record_success(&scan(vec![credential(5)]));

    let response = router.oneshot(Request::get("/metrics").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain; version=0.0.4; charset=utf-8");
    let body = String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(body.contains("esw_credential_days_left{"));
}

#[tokio::test]
async fn scan_endpoint_does_not_exist_without_secret() {
    let (router, started, _) = app("");
    let (status, _) = call(router, post_scan(Some(SECRET))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(started.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn scan_is_accepted_with_the_right_token() {
    let (router, started, _) = app(SECRET);
    let (status, _) = call(router, post_scan(Some(SECRET))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(started.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn scan_is_refused_with_a_wrong_or_missing_token() {
    for token in [None, Some(""), Some("wrong"), Some("a-long-random-secre")] {
        let (router, started, _) = app(SECRET);
        let (status, _) = call(router, post_scan(token)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "token {token:?}");
        assert_eq!(started.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn authorization_needs_both_sides_non_empty() {
    assert!(is_authorized(Some(b"abc"), "abc"));
    assert!(!is_authorized(Some(b"abc"), ""));
    assert!(!is_authorized(Some(b""), ""));
    assert!(!is_authorized(None, "abc"));
}
