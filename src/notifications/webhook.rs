use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::{debug, error};

use super::NotificationChannel;
use crate::config::WebhookOptions;
use crate::models::{ExpiringCredential, ScanResult};

pub const EVENT_NAME: &str = "credential.expiring";

/// Generic webhook: one JSON POST per expiring credential, so the receiver (Jira Automation,
/// n8n, Power Automate...) can deduplicate and track each credential on its own, using
/// `credential.keyId` as a stable identifier.
pub struct WebhookChannel {
    options: WebhookOptions,
    http: reqwest::Client,
}

impl WebhookChannel {
    pub fn new(options: WebhookOptions, http: reqwest::Client) -> Self {
        Self { options, http }
    }
}

#[async_trait]
impl NotificationChannel for WebhookChannel {
    fn name(&self) -> &'static str {
        "Webhook"
    }

    fn is_enabled(&self) -> bool {
        self.options.enabled
    }

    async fn send(&self, result: &ScanResult) -> Result<()> {
        if self.options.url.trim().is_empty() {
            return Ok(());
        }

        let mut failures = 0;
        for credential in &result.credentials {
            // One failing credential must not prevent the others from being sent
            if let Err(e) = self.post(result, credential).await {
                error!("Webhook call failed for credential {} of {}: {e:#}", credential.key_id, credential.app_name);
                failures += 1;
            }
        }

        if failures > 0 {
            bail!("Webhook failed for {failures} of {} credential(s).", result.credentials.len());
        }

        debug!("Webhook called for {} credential(s)", result.credentials.len());
        Ok(())
    }
}

impl WebhookChannel {
    async fn post(&self, result: &ScanResult, credential: &ExpiringCredential) -> Result<()> {
        let mut request = self.http.post(&self.options.url).json(&build_payload(result, credential));
        if !self.options.secret.is_empty() {
            request = request.header(&self.options.secret_header_name, &self.options.secret);
        }
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            // The receiver usually explains a rejection in the body, which error_for_status() would drop
            let body = response.text().await.unwrap_or_default();
            let body: String = body.chars().take(500).collect();
            bail!("HTTP status {status} for url ({}): {body}", self.options.url);
        }
        Ok(())
    }
}

pub(crate) fn build_payload(result: &ScanResult, credential: &ExpiringCredential) -> Value {
    json!({
        "event": EVENT_NAME,
        "tenantName": result.tenant_name,
        "tenantId": result.tenant_id,
        "scannedAt": result.scanned_at,
        "credential": {
            "keyId": credential.key_id,
            "appId": credential.app_id,
            "appName": credential.app_name,
            "type": credential.credential_type,
            "name": credential.credential_name,
            "expiresOn": credential.expires_on,
            "daysLeft": credential.days_left,
            "status": credential.status()
        }
    })
}

#[cfg(test)]
mod tests;
