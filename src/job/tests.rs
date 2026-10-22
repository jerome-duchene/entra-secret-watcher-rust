use super::*;
use crate::config::{EntraOptions, WebhookOptions};
use crate::graph::GraphClient;
use crate::notifications::NotificationChannel;
use crate::notifications::webhook::WebhookChannel;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn entra() -> EntraOptions {
    EntraOptions {
        tenant_id: "tenant".into(),
        client_id: "client".into(),
        client_secret: "secret".into(),
        tenant_name: "Contoso".into(),
    }
}

fn job(graph_server: &MockServer, hook_server: &MockServer, dry_run: bool) -> (CredentialScanJob, Arc<ScanMetrics>) {
    let options = WatcherOptions { dry_run, ..Default::default() };
    let graph = Arc::new(GraphClient::with_base_urls(
        reqwest::Client::new(),
        entra(),
        &format!("{}/graph", graph_server.uri()),
        &graph_server.uri(),
    ));
    let webhook = WebhookChannel::new(
        WebhookOptions { enabled: true, url: hook_server.uri(), ..Default::default() },
        reqwest::Client::new(),
    );
    let metrics = Arc::new(ScanMetrics::new("Contoso", "tenant"));
    let job = CredentialScanJob::new(
        GraphCredentialScanner::new(graph, entra(), options.clone()),
        NotificationDispatcher::new(vec![Arc::new(webhook) as Arc<dyn NotificationChannel>], true),
        metrics.clone(),
        options,
    );
    (job, metrics)
}

async fn mock_graph(server: &MockServer, end_date_time: &str) {
    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "t", "expires_in": 3600 })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/graph/applications"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [{
                "displayName": "App",
                "appId": "app-id",
                "passwordCredentials": [{ "keyId": "k1", "displayName": "s", "endDateTime": end_date_time }],
                "keyCredentials": []
            }]
        })))
        .mount(server)
        .await;
}

async fn mock_hook(server: &MockServer) {
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(server).await;
}

fn in_days(days: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::days(days) + chrono::Duration::hours(1)).to_rfc3339()
}

#[tokio::test]
async fn expiring_credentials_are_notified_and_published() {
    let (graph, hook) = (MockServer::start().await, MockServer::start().await);
    mock_graph(&graph, &in_days(5)).await;
    mock_hook(&hook).await;
    let (job, metrics) = job(&graph, &hook, false);

    job.execute().await.unwrap();

    assert_eq!(hook.received_requests().await.unwrap().len(), 1);
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.last_scan_success, Some(true));
    assert_eq!(snapshot.expiring_soon, 1);
    assert_eq!(snapshot.apps_scanned, 1);
}

#[tokio::test]
async fn nothing_is_sent_when_no_credential_is_expiring() {
    let (graph, hook) = (MockServer::start().await, MockServer::start().await);
    mock_graph(&graph, &in_days(200)).await;
    mock_hook(&hook).await;
    let (job, metrics) = job(&graph, &hook, false);

    job.execute().await.unwrap();

    assert!(hook.received_requests().await.unwrap().is_empty());
    assert_eq!(metrics.snapshot().last_scan_success, Some(true));
}

#[tokio::test]
async fn dry_run_does_not_notify_but_still_publishes_metrics() {
    let (graph, hook) = (MockServer::start().await, MockServer::start().await);
    mock_graph(&graph, &in_days(-3)).await;
    mock_hook(&hook).await;
    let (job, metrics) = job(&graph, &hook, true);

    job.execute().await.unwrap();

    assert!(hook.received_requests().await.unwrap().is_empty());
    assert_eq!(metrics.snapshot().expired, 1);
}

#[tokio::test]
async fn a_scan_failure_is_recorded_and_returned() {
    let (graph, hook) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(401)).mount(&graph).await;
    let (job, metrics) = job(&graph, &hook, false);

    assert!(job.execute().await.is_err());

    assert_eq!(metrics.snapshot().last_scan_success, Some(false));
    assert!(hook.received_requests().await.unwrap().is_empty());
}
