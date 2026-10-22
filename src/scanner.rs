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
mod tests {
    use super::*;
    use crate::graph::GraphCredential;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 17, 12, 0, 0).unwrap()
    }

    fn credential(key: &str, name: Option<&str>, ends_in: Option<Duration>) -> GraphCredential {
        GraphCredential {
            key_id: Some(key.into()),
            display_name: name.map(Into::into),
            end_date_time: ends_in.map(|d| now() + d),
        }
    }

    fn app(passwords: Vec<GraphCredential>, keys: Vec<GraphCredential>) -> GraphApplication {
        GraphApplication {
            display_name: Some("App".into()),
            app_id: Some("app-id".into()),
            password_credentials: Some(passwords),
            key_credentials: Some(keys),
        }
    }

    #[test]
    fn keeps_only_credentials_within_the_threshold() {
        let app = app(
            vec![
                credential("near", Some("near"), Some(Duration::days(10))),
                credential("edge", Some("edge"), Some(Duration::days(30) + Duration::hours(5))),
                credential("far", Some("far"), Some(Duration::days(31) + Duration::hours(1))),
            ],
            vec![],
        );
        let found = expiring_credentials(&app, now(), 30);
        let keys: Vec<_> = found.iter().map(|c| c.key_id.as_str()).collect();
        assert_eq!(keys, ["near", "edge"]);
    }

    #[test]
    fn days_left_truncates_toward_zero() {
        let app = app(
            vec![
                credential("past", None, Some(-Duration::hours(36))),
                credential("soon", None, Some(Duration::hours(36))),
            ],
            vec![],
        );
        let found = expiring_credentials(&app, now(), 30);
        assert_eq!(found[0].days_left, -1);
        assert_eq!(found[1].days_left, 1);
    }

    #[test]
    fn secrets_and_certificates_are_typed() {
        let app = app(
            vec![credential("s", Some("secret"), Some(Duration::days(1)))],
            vec![credential("c", Some("cert"), Some(Duration::days(2)))],
        );
        let found = expiring_credentials(&app, now(), 30);
        assert_eq!(found[0].credential_type, CredentialType::Secret);
        assert_eq!(found[1].credential_type, CredentialType::Certificate);
    }

    #[test]
    fn credentials_without_end_date_are_ignored() {
        let app = app(vec![credential("s", None, None)], vec![]);
        assert!(expiring_credentials(&app, now(), 30).is_empty());
    }

    #[test]
    fn missing_names_get_placeholders() {
        let app = GraphApplication {
            display_name: None,
            app_id: None,
            password_credentials: Some(vec![GraphCredential {
                key_id: None,
                display_name: None,
                end_date_time: Some(now() + Duration::days(1)),
            }]),
            key_credentials: None,
        };
        let found = expiring_credentials(&app, now(), 30);
        assert_eq!(found[0].app_name, "Unknown");
        assert_eq!(found[0].app_id, "Unknown");
        assert_eq!(found[0].credential_name, "Unnamed");
        assert_eq!(found[0].key_id, "");
    }
}
