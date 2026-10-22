use super::*;
use crate::config::EntraOptions;
use crate::models::test_support::{credential, scan};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn html_contains_summary_and_rows() {
    let html = build_html(&scan(vec![credential(-2), credential(8)]));
    assert!(html.contains("<strong>Tenant:</strong> Contoso"));
    assert!(html.contains("2026-10-17 03:00 UTC"));
    assert!(html.contains("<strong>1</strong> credential(s) already expired!"));
    assert!(html.contains("EXPIRED since 2 day(s)"));
    assert!(html.contains("Expires in 8 day(s)"));
    assert!(html.contains("#fdecea")); // expired row
    assert!(html.contains("#fff8e1")); // expiring row
}

#[test]
fn expired_banner_is_omitted_when_nothing_expired() {
    assert!(!build_html(&scan(vec![credential(8)])).contains("already expired"));
}

#[test]
fn user_controlled_values_are_escaped() {
    let mut cred = credential(5);
    cred.app_name = "<script>alert(1)</script>".into();
    let html = build_html(&scan(vec![cred]));
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}

#[tokio::test]
async fn sends_one_mail_to_all_recipients() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "t", "expires_in": 3600 })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graph/users/from%40contoso.com/sendMail"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;

    let graph = Arc::new(GraphClient::with_base_urls(
        reqwest::Client::new(),
        EntraOptions {
            tenant_id: "tenant".into(),
            client_id: "c".into(),
            client_secret: "s".into(),
            tenant_name: "x".into(),
        },
        &format!("{}/graph", server.uri()),
        &server.uri(),
    ));
    let channel = EmailChannel::new(
        EmailOptions { enabled: true, from: "from@contoso.com".into(), to: " a@x.com, ,b@x.com ".into() },
        graph,
    );
    channel.send(&scan(vec![credential(3)])).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let mail = requests.iter().find(|r| r.url.path().ends_with("sendMail")).unwrap();
    let body: Value = serde_json::from_slice(&mail.body).unwrap();
    let recipients: Vec<_> = body["message"]["toRecipients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["emailAddress"]["address"].as_str().unwrap())
        .collect();
    assert_eq!(recipients, ["a@x.com", "b@x.com"]);
    assert_eq!(body["message"]["subject"], "🔐 Entra ID Credential Report — Contoso (1 expiring)");
}
