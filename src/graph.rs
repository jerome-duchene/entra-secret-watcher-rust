//! Minimal Microsoft Graph client (client-credentials flow over plain HTTP) covering the two calls we need.
//!
//! No retry on purpose: `sendMail` is not idempotent, a retry could send a duplicate email.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::config::EntraOptions;

const GRAPH_URL: &str = "https://graph.microsoft.com/v1.0";
const LOGIN_URL: &str = "https://login.microsoftonline.com";

#[derive(Debug, Deserialize)]
struct GraphPage {
    #[serde(default)]
    value: Vec<GraphApplication>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphApplication {
    pub display_name: Option<String>,
    pub app_id: Option<String>,
    pub password_credentials: Option<Vec<GraphCredential>>,
    pub key_credentials: Option<Vec<GraphCredential>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCredential {
    pub key_id: Option<String>,
    pub display_name: Option<String>,
    pub end_date_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

struct CachedToken {
    value: String,
    valid_until: Instant,
}

pub struct GraphClient {
    http: reqwest::Client,
    entra: EntraOptions,
    graph_url: String,
    login_url: String,
    token: Mutex<Option<CachedToken>>,
}

impl GraphClient {
    pub fn new(http: reqwest::Client, entra: EntraOptions) -> Self {
        Self::with_base_urls(http, entra, GRAPH_URL, LOGIN_URL)
    }

    pub fn with_base_urls(http: reqwest::Client, entra: EntraOptions, graph_url: &str, login_url: &str) -> Self {
        Self {
            http,
            entra,
            graph_url: graph_url.trim_end_matches('/').to_string(),
            login_url: login_url.trim_end_matches('/').to_string(),
            token: Mutex::new(None),
        }
    }

    /// Every app registration with its secrets and certificates, following the pagination.
    pub async fn get_applications(&self) -> Result<Vec<GraphApplication>> {
        let mut applications = Vec::new();
        let mut url = Some(format!(
            "{}/applications?$select=displayName,appId,passwordCredentials,keyCredentials&$top=999",
            self.graph_url
        ));

        while let Some(current) = url {
            let response = self.send(self.http.get(&current)).await?;
            let page: GraphPage = response.json().await.context("Invalid Graph applications response")?;
            applications.extend(page.value);
            url = page.next_link;
        }
        Ok(applications)
    }

    pub async fn send_mail(&self, from: &str, subject: &str, html: &str, to: &[String]) -> Result<()> {
        let url = format!("{}/users/{}/sendMail", self.graph_url, urlencode(from));
        let body = json!({
            "message": {
                "subject": subject,
                "body": { "contentType": "HTML", "content": html },
                "toRecipients": to.iter()
                    .map(|address| json!({ "emailAddress": { "address": address } }))
                    .collect::<Vec<Value>>()
            },
            "saveToSentItems": false
        });
        self.send(self.http.post(url).json(&body)).await?;
        Ok(())
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let token = self.access_token().await?;
        let response = request.bearer_auth(token).send().await.context("Graph request failed")?;
        let status = response.status();
        if !status.is_success() {
            let method_and_url = response.url().path().to_string();
            let body = response.text().await.unwrap_or_default();
            bail!("Graph call to {method_and_url} failed: {} {body}", status.as_u16());
        }
        Ok(response)
    }

    async fn access_token(&self) -> Result<String> {
        let mut cached = self.token.lock().await;
        if let Some(token) = cached.as_ref().filter(|t| t.valid_until > Instant::now()) {
            return Ok(token.value.clone());
        }

        let response = self
            .http
            .post(format!("{}/{}/oauth2/v2.0/token", self.login_url, urlencode(&self.entra.tenant_id)))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.entra.client_id.as_str()),
                ("client_secret", self.entra.client_secret.as_str()),
                ("scope", "https://graph.microsoft.com/.default"),
            ])
            .send()
            .await
            .context("Token request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("Token request failed: {} {body}", status.as_u16());
        }

        let token: TokenResponse = response.json().await.context("Invalid token response")?;
        // Renew a minute early so a token never expires mid-request
        let lifetime = Duration::from_secs(token.expires_in.saturating_sub(60));
        *cached = Some(CachedToken { value: token.access_token.clone(), valid_until: Instant::now() + lifetime });
        Ok(token.access_token)
    }
}

/// Percent-encodes everything except unreserved characters (RFC 3986), like `Uri.EscapeDataString`.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn entra() -> EntraOptions {
        EntraOptions {
            tenant_id: "my-tenant".into(),
            client_id: "my-client".into(),
            client_secret: "my-secret".into(),
            tenant_name: "Contoso".into(),
        }
    }

    fn client(server: &MockServer) -> GraphClient {
        GraphClient::with_base_urls(reqwest::Client::new(), entra(), &format!("{}/graph", server.uri()), &server.uri())
    }

    async fn mock_token(server: &MockServer, expires_in: u64) {
        Mock::given(method("POST"))
            .and(path("/my-tenant/oauth2/v2.0/token"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=my-client"))
            .and(body_string_contains("client_secret=my-secret"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "access_token": "tok", "expires_in": expires_in })),
            )
            .mount(server)
            .await;
    }

    #[test]
    fn urlencode_escapes_reserved_characters() {
        assert_eq!(urlencode("noreply@fabrikam.com"), "noreply%40fabrikam.com");
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
        assert_eq!(urlencode("safe-_.~"), "safe-_.~");
    }

    #[tokio::test]
    async fn follows_pagination_and_parses_credentials() {
        let server = MockServer::start().await;
        mock_token(&server, 3600).await;

        let next = format!("{}/graph/applications-page-2", server.uri());
        Mock::given(method("GET"))
            .and(path("/graph/applications"))
            .and(query_param("$top", "999"))
            .and(header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": [{
                    "displayName": "App one",
                    "appId": "app-1",
                    "passwordCredentials": [
                        { "keyId": "11111111-1111-1111-1111-111111111111", "displayName": "s1", "endDateTime": "2026-11-01T00:00:00Z" }
                    ],
                    "keyCredentials": []
                }],
                "@odata.nextLink": next
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/graph/applications-page-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": [{ "displayName": null, "appId": "app-2", "passwordCredentials": null, "keyCredentials": [
                    { "keyId": null, "displayName": null, "endDateTime": null }
                ] }]
            })))
            .mount(&server)
            .await;

        let apps = client(&server).get_applications().await.unwrap();

        assert_eq!(apps.len(), 2);
        let first = &apps[0].password_credentials.as_ref().unwrap()[0];
        assert_eq!(first.key_id.as_deref(), Some("11111111-1111-1111-1111-111111111111"));
        assert_eq!(first.end_date_time.unwrap().to_rfc3339(), "2026-11-01T00:00:00+00:00");
        assert!(apps[1].display_name.is_none());
        assert!(apps[1].key_credentials.as_ref().unwrap()[0].end_date_time.is_none());
    }

    #[tokio::test]
    async fn token_is_cached_between_calls() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/my-tenant/oauth2/v2.0/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "access_token": "tok", "expires_in": 3600 })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/graph/applications"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;

        let graph = client(&server);
        graph.get_applications().await.unwrap();
        graph.get_applications().await.unwrap();
        // `expect(1)` is verified when the server is dropped
    }

    #[tokio::test]
    async fn expired_token_is_renewed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/my-tenant/oauth2/v2.0/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "tok", "expires_in": 30 })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/graph/applications"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;

        let graph = client(&server);
        graph.get_applications().await.unwrap();
        graph.get_applications().await.unwrap();
    }

    #[tokio::test]
    async fn token_failure_is_reported_with_status_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid_client"))
            .mount(&server)
            .await;

        let error = client(&server).get_applications().await.unwrap_err().to_string();
        assert_eq!(error, "Token request failed: 401 invalid_client");
    }

    #[tokio::test]
    async fn graph_failure_is_reported_with_status_and_body() {
        let server = MockServer::start().await;
        mock_token(&server, 3600).await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Insufficient privileges"))
            .mount(&server)
            .await;

        let error = client(&server).get_applications().await.unwrap_err().to_string();
        assert!(error.contains("403 Insufficient privileges"), "{error}");
    }

    #[tokio::test]
    async fn send_mail_posts_the_expected_message() {
        let server = MockServer::start().await;
        mock_token(&server, 3600).await;
        Mock::given(method("POST"))
            .and(path("/graph/users/noreply%40fabrikam.com/sendMail"))
            .and(header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&server)
            .await;

        client(&server)
            .send_mail("noreply@fabrikam.com", "Subject", "<b>hi</b>", &["a@b.c".to_string(), "d@e.f".to_string()])
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let mail = requests.iter().find(|r| r.url.path().ends_with("sendMail")).unwrap();
        let body: Value = serde_json::from_slice(&mail.body).unwrap();
        assert_eq!(body["message"]["subject"], "Subject");
        assert_eq!(body["message"]["body"]["contentType"], "HTML");
        assert_eq!(body["message"]["toRecipients"][1]["emailAddress"]["address"], "d@e.f");
        assert_eq!(body["saveToSentItems"], false);
    }
}
