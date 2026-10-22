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
mod tests;
