use std::sync::Arc;

use anyhow::Result;
use tracing::{error, info, instrument, warn};

use crate::config::WatcherOptions;
use crate::metrics::ScanMetrics;
use crate::notifications::NotificationDispatcher;
use crate::scanner::GraphCredentialScanner;

/// One scan: query Graph, publish the metrics, then notify (or only log in dry-run mode).
pub struct CredentialScanJob {
    scanner: GraphCredentialScanner,
    dispatcher: NotificationDispatcher,
    metrics: Arc<ScanMetrics>,
    options: WatcherOptions,
}

impl CredentialScanJob {
    pub fn new(
        scanner: GraphCredentialScanner,
        dispatcher: NotificationDispatcher,
        metrics: Arc<ScanMetrics>,
        options: WatcherOptions,
    ) -> Self {
        Self { scanner, dispatcher, metrics, options }
    }

    #[instrument(
        name = "CredentialScanJob.Execute",
        skip(self),
        fields(scan.credentials_found = tracing::field::Empty, scan.apps_scanned = tracing::field::Empty)
    )]
    pub async fn execute(&self) -> Result<()> {
        info!("Credential scan job started (DryRun: {})", self.options.dry_run);

        let result = match self.scanner.scan().await {
            Ok(result) => result,
            Err(e) => {
                self.metrics.record_failure();
                error!("Credential scan job failed: {e:#}");
                return Err(e);
            }
        };
        self.metrics.record_success(&result);

        tracing::Span::current().record("scan.credentials_found", result.credentials.len());
        tracing::Span::current().record("scan.apps_scanned", result.total_applications_scanned);

        if !result.has_expiring() {
            info!("No expiring credentials found. All clear! ✅");
            return Ok(());
        }

        warn!(
            "Found {} expiring credential(s): {} expired, {} expiring soon",
            result.credentials.len(),
            result.expired_count(),
            result.expiring_soon_count()
        );

        if self.options.dry_run {
            info!("[DRY-RUN] Would send notifications for {} credential(s). Skipping.", result.credentials.len());
            for c in &result.credentials {
                info!(
                    "[DRY-RUN] {} — {} '{}' — {}",
                    c.app_name,
                    c.credential_type,
                    c.credential_name,
                    c.status_label()
                );
            }
            return Ok(());
        }

        self.dispatcher.dispatch(&result).await;

        info!("Credential scan job completed successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests;
