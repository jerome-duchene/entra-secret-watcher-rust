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
mod tests {
    use super::*;
    use crate::models::test_support::{credential, scan};
    use std::sync::Mutex;

    struct FakeChannel {
        enabled: bool,
        fail: bool,
        sent: Mutex<Vec<usize>>,
    }

    impl FakeChannel {
        fn new(enabled: bool, fail: bool) -> Arc<Self> {
            Arc::new(Self { enabled, fail, sent: Mutex::new(vec![]) })
        }
        fn sent(&self) -> Vec<usize> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl NotificationChannel for FakeChannel {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn is_enabled(&self) -> bool {
            self.enabled
        }
        async fn send(&self, result: &ScanResult) -> Result<()> {
            self.sent.lock().unwrap().push(result.credentials.len());
            if self.fail { anyhow::bail!("boom") } else { Ok(()) }
        }
    }

    fn dispatcher(channels: &[&Arc<FakeChannel>], grouped: bool) -> NotificationDispatcher {
        NotificationDispatcher::new(
            channels.iter().map(|c| (*c).clone() as Arc<dyn NotificationChannel>).collect(),
            grouped,
        )
    }

    #[tokio::test]
    async fn grouped_report_sends_one_batch() {
        let channel = FakeChannel::new(true, false);
        let result = scan(vec![credential(1), credential(2), credential(3)]);
        dispatcher(&[&channel], true).dispatch(&result).await;
        assert_eq!(channel.sent(), vec![3]);
    }

    #[tokio::test]
    async fn ungrouped_report_sends_one_batch_per_credential() {
        let channel = FakeChannel::new(true, false);
        let result = scan(vec![credential(1), credential(2), credential(3)]);
        dispatcher(&[&channel], false).dispatch(&result).await;
        assert_eq!(channel.sent(), vec![1, 1, 1]);
    }

    #[tokio::test]
    async fn disabled_channels_are_skipped() {
        let enabled = FakeChannel::new(true, false);
        let disabled = FakeChannel::new(false, false);
        dispatcher(&[&enabled, &disabled], true).dispatch(&scan(vec![credential(1)])).await;
        assert_eq!(enabled.sent().len(), 1);
        assert!(disabled.sent().is_empty());
    }

    #[tokio::test]
    async fn a_failing_channel_does_not_stop_the_others() {
        let failing = FakeChannel::new(true, true);
        let healthy = FakeChannel::new(true, false);
        dispatcher(&[&failing, &healthy], true).dispatch(&scan(vec![credential(1)])).await;
        assert_eq!(failing.sent().len(), 1);
        assert_eq!(healthy.sent().len(), 1);
    }

    #[tokio::test]
    async fn nothing_is_sent_without_enabled_channel() {
        let disabled = FakeChannel::new(false, false);
        dispatcher(&[&disabled], true).dispatch(&scan(vec![credential(1)])).await;
        assert!(disabled.sent().is_empty());
    }

    #[test]
    fn html_escape_neutralises_markup() {
        assert_eq!(html_escape(r#"<a href="x">&'"#), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    }
}
