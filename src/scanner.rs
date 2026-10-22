use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tracing::{info, instrument};

use crate::config::{EntraOptions, WatcherOptions};
use crate::graph::{GraphApplication, GraphClient};
use crate::models::{CredentialType, ExpiringCredential, ScanResult};

pub struct GraphCredentialScanner {
    graph: Arc<GraphClient>,
    entra: EntraOptions,
    watcher: WatcherOptions,
}

impl GraphCredentialScanner {
    pub fn new(graph: Arc<GraphClient>, entra: EntraOptions, watcher: WatcherOptions) -> Self {
        Self { graph, entra, watcher }
    }

    #[instrument(name = "ScanCredentials", skip(self), fields(tenant.id = %self.entra.tenant_id, tenant.name = %self.entra.tenant_name))]
    pub async fn scan(&self) -> Result<ScanResult> {
        info!("Starting credential scan for tenant {} ({})", self.entra.tenant_name, self.entra.tenant_id);

        let applications = self.graph.get_applications().await?;
        let now = Utc::now();
        let total_apps = applications.len();

        let mut expiring: Vec<ExpiringCredential> =
            applications.iter().flat_map(|app| expiring_credentials(app, now, self.watcher.threshold_days)).collect();
        expiring.sort_by_key(|c| c.days_left);

        info!(
            "Scan complete for {}: {} apps scanned, {} credential(s) expiring within {} days",
            self.entra.tenant_name,
            total_apps,
            expiring.len(),
            self.watcher.threshold_days
        );

        Ok(ScanResult {
            tenant_name: self.entra.tenant_name.clone(),
            tenant_id: self.entra.tenant_id.clone(),
            scanned_at: now,
            credentials: expiring,
            total_applications_scanned: total_apps,
        })
    }
}

fn expiring_credentials(app: &GraphApplication, now: DateTime<Utc>, threshold_days: i64) -> Vec<ExpiringCredential> {
    [(CredentialType::Secret, &app.password_credentials), (CredentialType::Certificate, &app.key_credentials)]
        .into_iter()
        .flat_map(|(credential_type, credentials)| {
            credentials.iter().flatten().filter_map(move |credential| {
                let end_date = credential.end_date_time?;
                // Truncated toward zero, like TimeSpan.Days
                let days_left = (end_date - now).num_days();
                (days_left <= threshold_days).then(|| ExpiringCredential {
                    app_name: app.display_name.clone().unwrap_or_else(|| "Unknown".into()),
                    app_id: app.app_id.clone().unwrap_or_else(|| "Unknown".into()),
                    key_id: credential.key_id.clone().unwrap_or_default(),
                    credential_type,
                    credential_name: credential.display_name.clone().unwrap_or_else(|| "Unnamed".into()),
                    expires_on: end_date,
                    days_left,
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
