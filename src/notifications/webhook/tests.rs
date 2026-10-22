use super::*;
use crate::models::test_support::{credential, scan};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn options(url: String, secret: &str) -> WebhookOptions {
    WebhookOptions { enabled: true, url, secret: secret.into(), secret_header_name: "X-Automation-Token".into() }
}

#[test]
fn payload_matches_the_documented_contract() {
    let payload = build_payload(&scan(vec![]), &credential(8));
    assert_eq!(
        payload,
        json!({
            "event": "credential.expiring",
            "tenantName": "Contoso",
            "tenantId": "tenant-id",
            "scannedAt": "2026-10-17T03:00:00Z",
            "credential": {
                "keyId": "key-id",
                "appId": "app-id",
                "appName": "My App",
                "type": "secret",
                "name": "prod-secret",
                "expiresOn": "2026-10-25T00:00:00Z",
                "daysLeft": 8,
                "status": "expiringSoon"
            }
        })
    );
}

#[test]
fn expired_status_is_serialised_in_camel_case() {
    let payload = build_payload(&scan(vec![]), &credential(-1));
    assert_eq!(payload["credential"]["status"], "expired");
}

#[tokio::test]
async fn sends_one_post_per_credential_with_the_secret_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .and(header("x-automation-token", "shh"))
        .respond_with(ResponseTemplate::new(200))
        .expect(2)
        .mount(&server)
        .await;

    let channel = WebhookChannel::new(options(format!("{}/hook", server.uri()), "shh"), reqwest::Client::new());
    channel.send(&scan(vec![credential(1), credential(2)])).await.unwrap();
}

#[tokio::test]
async fn no_secret_header_without_secret() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;

    let channel = WebhookChannel::new(options(server.uri(), ""), reqwest::Client::new());
    channel.send(&scan(vec![credential(1)])).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert!(!requests[0].headers.contains_key("x-automation-token"));
}

#[tokio::test]
async fn one_failure_does_not_stop_the_others_but_is_reported() {
    let server = MockServer::start().await;
    let mut first = credential(1);
    first.key_id = "bad".into();
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains("\"keyId\":\"bad\""))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;

    let channel = WebhookChannel::new(options(server.uri(), ""), reqwest::Client::new());
    let error = channel.send(&scan(vec![first, credential(2)])).await.unwrap_err();

    assert_eq!(error.to_string(), "Webhook failed for 1 of 2 credential(s).");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
