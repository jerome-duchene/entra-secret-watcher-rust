//! End-to-end scenario through the public API only: environment-style settings → scan job →
//! mocked Graph and webhook → Prometheus output. No private item is reachable from here.

use std::sync::Arc;

use chrono::{Duration, Utc};
use entra_secret_watcher::config::{Config, Settings};
use entra_secret_watcher::graph::GraphClient;
use entra_secret_watcher::job::CredentialScanJob;
use entra_secret_watcher::metrics::ScanMetrics;
use entra_secret_watcher::notifications::webhook::WebhookChannel;
use entra_secret_watcher::notifications::{NotificationChannel, NotificationDispatcher};
use entra_secret_watcher::scanner::GraphCredentialScanner;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn config(webhook_url: &str) -> Config {
    Config::from_settings(&Settings::from_pairs([
        ("Entra__TenantId", "tenant"),
        ("Entra__ClientId", "client"),
        ("Entra__ClientSecret", "secret"),
        ("Entra__TenantName", "Contoso"),
        ("Watcher__ThresholdDays", "30"),
        ("Notification__Webhook__Enabled", "true"),
        ("Notification__Webhook__Url", webhook_url),
        ("Notification__Webhook__Secret", "shh"),
    ]))
    .expect("valid configuration")
}

fn end_date(days: i64) -> String {
    (Utc::now() + Duration::days(days) + Duration::hours(1)).to_rfc3339()
}

#[tokio::test]
async fn a_scan_notifies_the_webhook_and_publishes_the_metrics() {
    let microsoft = MockServer::start().await;
    let hook = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "t", "expires_in": 3600 })))
        .mount(&microsoft)
        .await;
    Mock::given(method("GET"))
        .and(path("/graph/applications"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [
                { "displayName": "Billing", "appId": "app-1",
                  "passwordCredentials": [
                      { "keyId": "k-expiring", "displayName": "prod", "endDateTime": end_date(5) },
                      { "keyId": "k-expired", "displayName": "old", "endDateTime": end_date(-10) },
                      { "keyId": "k-fine", "displayName": "next-year", "endDateTime": end_date(300) }
                  ],
                  "keyCredentials": [] },
                { "displayName": "Portal", "appId": "app-2", "passwordCredentials": [], "keyCredentials": [] }
            ]
        })))
        .mount(&microsoft)
        .await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&hook).await;

    let config = config(&hook.uri());
    let http = reqwest::Client::new();
    let graph = Arc::new(GraphClient::with_base_urls(
        http.clone(),
        config.entra.clone(),
        &format!("{}/graph", microsoft.uri()),
        &microsoft.uri(),
    ));
    let metrics = Arc::new(ScanMetrics::new(&config.entra.tenant_name, &config.entra.tenant_id));
    let channels: Vec<Arc<dyn NotificationChannel>> =
        vec![Arc::new(WebhookChannel::new(config.notification.webhook.clone(), http))];
    let job = CredentialScanJob::new(
        GraphCredentialScanner::new(graph, config.entra.clone(), config.watcher.clone()),
        NotificationDispatcher::new(channels, config.watcher.grouped_report),
        metrics.clone(),
        config.watcher.clone(),
    );

    job.execute().await.unwrap();

    // One webhook call per credential within the threshold (the one expiring next year is left out)
    let calls = hook.received_requests().await.unwrap();
    assert_eq!(calls.len(), 2);
    let mut payloads: Vec<Value> = calls.iter().map(|r| serde_json::from_slice(&r.body).unwrap()).collect();
    payloads.sort_by_key(|p| p["credential"]["keyId"].as_str().unwrap().to_string());
    assert_eq!(payloads[0]["credential"]["keyId"], "k-expired");
    assert_eq!(payloads[0]["credential"]["status"], "expired");
    assert_eq!(payloads[1]["credential"]["keyId"], "k-expiring");
    assert_eq!(payloads[1]["credential"]["status"], "expiringSoon");
    assert_eq!(payloads[1]["tenantName"], "Contoso");
    assert!(calls.iter().all(|r| r.headers.get("x-webhook-token").is_some_and(|v| v == "shh")));

    let prometheus = metrics.render_prometheus();
    assert!(prometheus.contains(r#"esw_apps_scanned{tenant_name="Contoso",tenant_id="tenant"} 2"#));
    assert!(prometheus.contains(r#"esw_credentials_expired{tenant_name="Contoso",tenant_id="tenant"} 1"#));
    assert!(prometheus.contains(r#"esw_credentials_expiring_soon{tenant_name="Contoso",tenant_id="tenant"} 1"#));
    assert!(prometheus.contains(r#"esw_last_scan_success{tenant_name="Contoso",tenant_id="tenant"} 1"#));
}

#[tokio::test]
async fn an_unreachable_tenant_flags_the_failure_without_notifying() {
    let microsoft = MockServer::start().await;
    let hook = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(400)).mount(&microsoft).await;

    let config = config(&hook.uri());
    let http = reqwest::Client::new();
    let graph = Arc::new(GraphClient::with_base_urls(
        http.clone(),
        config.entra.clone(),
        &format!("{}/graph", microsoft.uri()),
        &microsoft.uri(),
    ));
    let metrics = Arc::new(ScanMetrics::new("Contoso", "tenant"));
    let job = CredentialScanJob::new(
        GraphCredentialScanner::new(graph, config.entra.clone(), config.watcher.clone()),
        NotificationDispatcher::new(
            vec![Arc::new(WebhookChannel::new(config.notification.webhook.clone(), http))
                as Arc<dyn NotificationChannel>],
            true,
        ),
        metrics.clone(),
        config.watcher.clone(),
    );

    assert!(job.execute().await.is_err());

    assert!(hook.received_requests().await.unwrap().is_empty());
    assert!(
        metrics.render_prometheus().contains(r#"esw_last_scan_success{tenant_name="Contoso",tenant_id="tenant"} 0"#)
    );
}
