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
mod tests;
