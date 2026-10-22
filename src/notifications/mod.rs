pub mod email;
pub mod gotify;
pub mod teams;
pub mod webhook;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::{error, info, warn};

use crate::models::ScanResult;

#[async_trait]
pub trait NotificationChannel: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_enabled(&self) -> bool;
    async fn send(&self, result: &ScanResult) -> Result<()>;
}

/// Sends a scan result to every enabled channel. A failing channel never stops the others.
pub struct NotificationDispatcher {
    channels: Vec<Arc<dyn NotificationChannel>>,
    grouped_report: bool,
}

impl NotificationDispatcher {
    pub fn new(channels: Vec<Arc<dyn NotificationChannel>>, grouped_report: bool) -> Self {
        Self { channels, grouped_report }
    }

    pub async fn dispatch(&self, result: &ScanResult) {
        let enabled: Vec<_> = self.channels.iter().filter(|c| c.is_enabled()).collect();

        if enabled.is_empty() {
            warn!("No notification channels are enabled. Skipping notification dispatch.");
            return;
        }

        let batches: Vec<ScanResult> = if self.grouped_report {
            vec![result.clone()]
        } else {
            result.credentials.iter().map(|credential| result.with_credentials(vec![credential.clone()])).collect()
        };

        for batch in &batches {
            for channel in &enabled {
                info!("Sending notification via {}", channel.name());
                match channel.send(batch).await {
                    Ok(()) => info!("Notification sent successfully via {}", channel.name()),
                    Err(e) => error!("Failed to send notification via {}: {e:#}", channel.name()),
                }
            }
        }
    }
}

pub(crate) fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
