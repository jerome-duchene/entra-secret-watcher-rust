use super::*;
use crate::models::test_support::{credential, scan};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn card_has_summary_and_one_container_per_credential() {
    let card = build_adaptive_card(&scan(vec![credential(-2), credential(8)]));
    let content = &card["attachments"][0]["content"];

    assert_eq!(card["type"], "message");
    assert_eq!(content["type"], "AdaptiveCard");
    assert_eq!(content["version"], "1.4");

    let body = content["body"].as_array().unwrap();
    assert_eq!(body.len(), 4);
    let summary = &body[1]["facts"];
    assert_eq!(summary[0]["value"], "Contoso");
    assert_eq!(summary[1]["value"], "2026-10-17 03:00 UTC");
    assert_eq!(summary[2]["value"], "12");
    assert_eq!(summary[3]["value"], "1 ⚠️");
    assert_eq!(summary[4]["value"], "1 🔴");

    assert_eq!(body[2]["items"][0]["text"], "🔴 **My App**");
    assert_eq!(body[3]["items"][0]["text"], "🟠 **My App**");
    assert_eq!(body[2]["items"][1]["facts"][3]["value"], "EXPIRED since 2 day(s)");
}

#[test]
fn special_characters_stay_valid_json() {
    let mut cred = credential(5);
    cred.credential_name = "quote \" and \\ and\nnewline".into();
    let text = build_adaptive_card(&scan(vec![cred])).to_string();
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        parsed["attachments"][0]["content"]["body"][2]["items"][1]["facts"][1]["value"],
        "quote \" and \\ and\nnewline"
    );
}

#[tokio::test]
async fn posts_the_card_to_the_webhook() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let channel = TeamsChannel::new(
        TeamsOptions { enabled: true, webhook_url: format!("{}/hook", server.uri()) },
        reqwest::Client::new(),
    );
    channel.send(&scan(vec![credential(3)])).await.unwrap();
}

#[tokio::test]
async fn http_errors_are_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(400)).mount(&server).await;

    let channel = TeamsChannel::new(TeamsOptions { enabled: true, webhook_url: server.uri() }, reqwest::Client::new());
    assert!(channel.send(&scan(vec![credential(3)])).await.is_err());
}
