//! Metrics describing the outcome of the last scan.
//!
//! One snapshot is the single source of truth, published two ways: rendered as Prometheus text on
//! `/metrics`, and observed by the OpenTelemetry gauges pushed over OTLP (see `telemetry`).
//! Scans run rarely (cron) while metrics are read continuously, so the last values are kept
//! between two scans. Every series carries the `tenant_name` and `tenant_id` labels so that several
//! instances (one container per tenant) can be told apart whatever the collection path.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;

use crate::models::{ExpiringCredential, ScanResult};

pub const DAYS_LEFT: (&str, &str) = (
    "esw_credential_days_left",
    "Days left before expiration of a credential reported by the last scan (negative = expired).",
);
pub const EXPIRED: (&str, &str) = ("esw_credentials_expired", "Number of expired credentials found by the last scan.");
pub const EXPIRING_SOON: (&str, &str) =
    ("esw_credentials_expiring_soon", "Number of credentials about to expire found by the last scan.");
pub const APPS_SCANNED: (&str, &str) = ("esw_apps_scanned", "Number of app registrations inspected by the last scan.");
pub const LAST_SCAN_TIMESTAMP: (&str, &str) =
    ("esw_last_scan_timestamp_seconds", "Unix timestamp of the last scan attempt.");
pub const LAST_SCAN_SUCCESS: (&str, &str) = ("esw_last_scan_success", "1 if the last scan succeeded, 0 otherwise.");

// Application health: unlike the figures above, these say nothing about the credentials themselves.
pub const SCANS_TOTAL: (&str, &str) = ("esw_scans_total", "Scans run since the process started, by result.");
pub const NOTIFICATIONS_TOTAL: (&str, &str) =
    ("esw_notifications_total", "Notifications attempted since the process started, by channel and result.");
pub const GRAPH_REQUESTS_TOTAL: (&str, &str) =
    ("esw_graph_requests_total", "Microsoft Graph calls made since the process started, by result.");
/// The counters, in the order they are rendered.
pub const COUNTERS: [(&str, &str); 3] = [SCANS_TOTAL, NOTIFICATIONS_TOTAL, GRAPH_REQUESTS_TOTAL];
pub const LAST_SCAN_DURATION: (&str, &str) =
    ("esw_last_scan_duration_seconds", "Duration of the last scan attempt, in seconds.");
pub const PROCESS_START_TIME: (&str, &str) =
    ("esw_process_start_time_seconds", "Unix timestamp of the start of the process.");
pub const BUILD_INFO: (&str, &str) = ("esw_build_info", "Version of the running application (always 1).");

pub const RESULT_SUCCESS: &str = "success";
pub const RESULT_FAILURE: &str = "failure";

/// A counter series: its metric name and its labels, in a stable order.
pub type Series = (&'static str, Vec<(&'static str, String)>);

#[derive(Debug, Clone, Default)]
pub struct Activity {
    pub counters: BTreeMap<Series, u64>,
    pub last_scan_duration: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub credentials: Vec<ExpiringCredential>,
    pub expired: usize,
    pub expiring_soon: usize,
    pub apps_scanned: usize,
    pub last_scan_timestamp: i64,
    /// `None` until the first scan attempt: an absent series is clearer than a misleading 0.
    pub last_scan_success: Option<bool>,
    /// The figures only exist once a scan has succeeded.
    pub has_result: bool,
}

pub struct ScanMetrics {
    tenant_name: String,
    tenant_id: String,
    snapshot: RwLock<Snapshot>,
    activity: Mutex<Activity>,
    started_at: i64,
}

impl ScanMetrics {
    pub fn new(tenant_name: impl Into<String>, tenant_id: impl Into<String>) -> Self {
        Self {
            tenant_name: tenant_name.into(),
            tenant_id: tenant_id.into(),
            snapshot: RwLock::default(),
            activity: Mutex::default(),
            started_at: Utc::now().timestamp(),
        }
    }

    pub fn tenant_name(&self) -> &str {
        &self.tenant_name
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn activity(&self) -> Activity {
        self.activity.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn started_at(&self) -> i64 {
        self.started_at
    }

    pub fn count_scan(&self, success: bool) {
        self.count(SCANS_TOTAL.0, vec![("result", result_label(success).into())]);
    }

    pub fn count_notification(&self, channel: &str, success: bool) {
        self.count(NOTIFICATIONS_TOTAL.0, vec![("channel", channel.into()), ("result", result_label(success).into())]);
    }

    pub fn count_graph_request(&self, success: bool) {
        self.count(GRAPH_REQUESTS_TOTAL.0, vec![("result", result_label(success).into())]);
    }

    pub fn record_scan_duration(&self, duration: Duration) {
        self.activity.lock().unwrap_or_else(|e| e.into_inner()).last_scan_duration = Some(duration);
    }

    fn count(&self, metric: &'static str, labels: Vec<(&'static str, String)>) {
        *self.activity.lock().unwrap_or_else(|e| e.into_inner()).counters.entry((metric, labels)).or_default() += 1;
    }

    pub fn record_success(&self, result: &ScanResult) {
        // Replacing the whole snapshot drops the series of the previous scan: a renewed credential disappears
        *self.snapshot.write().unwrap_or_else(|e| e.into_inner()) = Snapshot {
            credentials: result.credentials.clone(),
            expired: result.expired_count(),
            expiring_soon: result.expiring_soon_count(),
            apps_scanned: result.total_applications_scanned,
            last_scan_timestamp: result.scanned_at.timestamp(),
            last_scan_success: Some(true),
            has_result: true,
        };
    }

    pub fn record_failure(&self) {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|e| e.into_inner());
        // Keep the figures of the last successful scan, only flag the failure
        snapshot.last_scan_timestamp = Utc::now().timestamp();
        snapshot.last_scan_success = Some(false);
    }

    /// Prometheus text exposition format (version 0.0.4).
    pub fn render_prometheus(&self) -> String {
        let s = self.snapshot();
        let tenant = [("tenant_name", self.tenant_name.as_str()), ("tenant_id", self.tenant_id.as_str())];
        let mut out = String::new();

        header(&mut out, DAYS_LEFT);
        for c in &s.credentials {
            let labels = [
                ("tenant_name", self.tenant_name.as_str()),
                ("tenant_id", self.tenant_id.as_str()),
                ("app_name", c.app_name.as_str()),
                ("app_id", c.app_id.as_str()),
                ("key_id", c.key_id.as_str()),
                ("type", c.credential_type.as_str()),
                ("credential_name", c.credential_name.as_str()),
            ];
            sample(&mut out, DAYS_LEFT.0, &labels, c.days_left);
        }

        let scalars = [
            (EXPIRED, s.has_result.then_some(s.expired as i64)),
            (EXPIRING_SOON, s.has_result.then_some(s.expiring_soon as i64)),
            (APPS_SCANNED, s.has_result.then_some(s.apps_scanned as i64)),
            (LAST_SCAN_TIMESTAMP, s.last_scan_success.map(|_| s.last_scan_timestamp)),
            (LAST_SCAN_SUCCESS, s.last_scan_success.map(i64::from)),
        ];
        for (metric, value) in scalars {
            header(&mut out, metric);
            if let Some(value) = value {
                sample(&mut out, metric.0, &tenant, value);
            }
        }

        let activity = self.activity();
        for metric in COUNTERS {
            header_of_kind(&mut out, metric, "counter");
            for ((_, series), value) in activity.counters.iter().filter(|((name, _), _)| *name == metric.0) {
                let labels: Vec<(&str, &str)> =
                    tenant.iter().copied().chain(series.iter().map(|(k, v)| (*k, v.as_str()))).collect();
                sample(&mut out, metric.0, &labels, value);
            }
        }

        header(&mut out, LAST_SCAN_DURATION);
        if let Some(duration) = activity.last_scan_duration {
            sample(&mut out, LAST_SCAN_DURATION.0, &tenant, duration.as_secs_f64());
        }
        header(&mut out, PROCESS_START_TIME);
        sample(&mut out, PROCESS_START_TIME.0, &tenant, self.started_at);
        header(&mut out, BUILD_INFO);
        let build = [tenant[0], tenant[1], ("version", env!("CARGO_PKG_VERSION"))];
        sample(&mut out, BUILD_INFO.0, &build, 1);
        out
    }
}

fn header(out: &mut String, metric: (&str, &str)) {
    header_of_kind(out, metric, "gauge");
}

fn header_of_kind(out: &mut String, (name, help): (&str, &str), kind: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn result_label(success: bool) -> &'static str {
    if success { RESULT_SUCCESS } else { RESULT_FAILURE }
}

fn sample(out: &mut String, name: &str, labels: &[(&str, &str)], value: impl std::fmt::Display) {
    let labels = labels.iter().map(|(k, v)| format!("{k}=\"{}\"", escape_label_value(v))).collect::<Vec<_>>().join(",");
    let _ = writeln!(out, "{name}{{{labels}}} {value}");
}

fn escape_label_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n").replace('"', "\\\"")
}

#[cfg(test)]
mod tests;
