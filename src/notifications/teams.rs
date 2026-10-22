use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::debug;

use super::NotificationChannel;
use crate::config::TeamsOptions;
use crate::models::{CredentialStatus, ScanResult};

/// Posts an Adaptive Card to a Teams Incoming Webhook (no Graph permission needed).
pub struct TeamsChannel {
    options: TeamsOptions,
    http: reqwest::Client,
}

impl TeamsChannel {
    pub fn new(options: TeamsOptions, http: reqwest::Client) -> Self {
        Self { options, http }
    }
}

#[async_trait]
impl NotificationChannel for TeamsChannel {
    fn name(&self) -> &'static str {
        "Teams (Webhook)"
    }

    fn is_enabled(&self) -> bool {
        self.options.enabled
    }

    async fn send(&self, result: &ScanResult) -> Result<()> {
        if self.options.webhook_url.is_empty() {
            return Ok(());
        }

        self.http
            .post(&self.options.webhook_url)
            .json(&build_adaptive_card(result))
            .send()
            .await
            .context("Teams request failed")?
            .error_for_status()
            .context("Teams rejected the notification")?;

        debug!("Teams webhook notification sent successfully");
        Ok(())
    }
}

pub(crate) fn build_adaptive_card(result: &ScanResult) -> Value {
    let mut body = vec![
        json!({
            "type": "TextBlock",
            "text": "🔐 Entra ID Credential Report",
            "weight": "Bolder",
            "size": "Large"
        }),
        json!({
            "type": "FactSet",
            "facts": [
                { "title": "Tenant", "value": result.tenant_name },
                { "title": "Scanned", "value": format!("{} UTC", result.scanned_at.format("%Y-%m-%d %H:%M")) },
                { "title": "Apps scanned", "value": result.total_applications_scanned.to_string() },
                { "title": "Expiring", "value": format!("{} ⚠️", result.expiring_soon_count()) },
                { "title": "Expired", "value": format!("{} 🔴", result.expired_count()) }
            ]
        }),
    ];

    body.extend(result.credentials.iter().map(|c| {
        let icon = if c.status() == CredentialStatus::Expired { "🔴" } else { "🟠" };
        json!({
            "type": "Container",
            "separator": true,
            "items": [
                { "type": "TextBlock", "text": format!("{icon} **{}**", c.app_name), "wrap": true },
                {
                    "type": "FactSet",
                    "facts": [
                        { "title": "Type", "value": c.credential_type.to_string() },
                        { "title": "Name", "value": c.credential_name },
                        { "title": "Expires", "value": c.expires_on.format("%Y-%m-%d").to_string() },
                        { "title": "Status", "value": c.status_label() }
                    ]
                }
            ]
        })
    }));

    json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "contentUrl": null,
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.4",
                "msteams": { "width": "Full" },
                "body": body
            }
        }]
    })
}

#[cfg(test)]
mod tests {
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

        let channel =
            TeamsChannel::new(TeamsOptions { enabled: true, webhook_url: server.uri() }, reqwest::Client::new());
        assert!(channel.send(&scan(vec![credential(3)])).await.is_err());
    }
}
