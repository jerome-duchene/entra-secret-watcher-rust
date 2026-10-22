use std::fmt::Write;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;
use tracing::debug;

use super::NotificationChannel;
use crate::config::GotifyOptions;
use crate::models::{CredentialStatus, ScanResult};

pub struct GotifyChannel {
    options: GotifyOptions,
    http: reqwest::Client,
}

impl GotifyChannel {
    pub fn new(options: GotifyOptions, http: reqwest::Client) -> Self {
        Self { options, http }
    }
}

#[async_trait]
impl NotificationChannel for GotifyChannel {
    fn name(&self) -> &'static str {
        "Gotify"
    }

    fn is_enabled(&self) -> bool {
        self.options.enabled
    }

    async fn send(&self, result: &ScanResult) -> Result<()> {
        let priority = compute_priority(result);
        let payload = json!({
            "title": format!(
                "🔐 Entra ID — {}: {} credential(s) expiring",
                result.tenant_name,
                result.credentials.len()
            ),
            "message": build_plain_text(result),
            "priority": priority,
        });

        // The token goes in a header rather than in the query string, so it never ends up in a logged URL
        self.http
            .post(format!("{}/message", self.options.url.trim_end_matches('/')))
            .header("X-Gotify-Key", &self.options.token)
            .json(&payload)
            .send()
            .await
            .context("Gotify request failed")?
            .error_for_status()
            .context("Gotify rejected the notification")?;

        debug!("Gotify notification sent with priority {priority}");
        Ok(())
    }
}

pub(crate) fn compute_priority(result: &ScanResult) -> u8 {
    if result.expired_count() > 0 {
        9
    } else if result.expiring_soon_count() > 0 {
        6
    } else {
        3
    }
}

pub(crate) fn build_plain_text(result: &ScanResult) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "🔐 Entra ID Credential Report — {}", result.tenant_name);
    let _ = writeln!(out, "Scanned at: {} UTC", result.scanned_at.format("%Y-%m-%d %H:%M"));
    let _ = writeln!(out, "Applications scanned: {}", result.total_applications_scanned);
    let _ = writeln!(
        out,
        "Credentials expiring: {} (Expired: {}, Expiring soon: {})",
        result.credentials.len(),
        result.expired_count(),
        result.expiring_soon_count()
    );
    let _ = writeln!(out, "{}", "─".repeat(50));

    for credential in &result.credentials {
        let icon = if credential.status() == CredentialStatus::Expired { "🔴" } else { "🟠" };
        let _ = writeln!(out, "{icon} {}", credential.app_name);
        let _ = writeln!(out, "   Type: {} | Name: {}", credential.credential_type, credential.credential_name);
        let _ =
            writeln!(out, "   {} (Expires: {})", credential.status_label(), credential.expires_on.format("%Y-%m-%d"));
        let _ = writeln!(out, "   AppId: {}", credential.app_id);
        let _ = writeln!(out);
    }
    out
}

#[cfg(test)]
mod tests {
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
}
