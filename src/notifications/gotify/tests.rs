use super::*;
use crate::models::test_support::{credential, scan};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn priority_depends_on_the_worst_status() {
    assert_eq!(compute_priority(&scan(vec![credential(-1), credential(5)])), 9);
    assert_eq!(compute_priority(&scan(vec![credential(5)])), 6);
    assert_eq!(compute_priority(&scan(vec![credential(60)])), 3);
    assert_eq!(compute_priority(&scan(vec![])), 3);
}

#[test]
fn plain_text_lists_every_credential() {
    let text = build_plain_text(&scan(vec![credential(-2), credential(8)]));
    assert!(text.starts_with("🔐 Entra ID Credential Report — Contoso\n"));
    assert!(text.contains("Scanned at: 2026-10-17 03:00 UTC"));
    assert!(text.contains("Applications scanned: 12"));
    assert!(text.contains("Credentials expiring: 2 (Expired: 1, Expiring soon: 1)"));
    assert!(text.contains("🔴 My App"));
    assert!(text.contains("🟠 My App"));
    assert!(text.contains("   Type: Secret | Name: prod-secret"));
    assert!(text.contains("   EXPIRED since 2 day(s) (Expires: 2026-10-25)"));
    assert!(text.contains("   Expires in 8 day(s) (Expires: 2026-10-25)"));
    assert!(text.contains("   AppId: app-id"));
}

#[tokio::test]
async fn posts_the_message_with_the_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/message"))
        .and(header("x-gotify-key", "app-token"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let channel = GotifyChannel::new(
        GotifyOptions { enabled: true, url: format!("{}/", server.uri()), token: "app-token".into() },
        reqwest::Client::new(),
    );
    channel.send(&scan(vec![credential(-1)])).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["priority"], 9);
    assert_eq!(body["title"], "🔐 Entra ID — Contoso: 1 credential(s) expiring");
}

#[tokio::test]
async fn http_errors_are_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(401)).mount(&server).await;

    let channel = GotifyChannel::new(
        GotifyOptions { enabled: true, url: server.uri(), token: "bad".into() },
        reqwest::Client::new(),
    );
    assert!(channel.send(&scan(vec![credential(1)])).await.is_err());
}
