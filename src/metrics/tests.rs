use super::*;
use crate::models::test_support::{credential, scan};

fn metrics() -> ScanMetrics {
    ScanMetrics::new("Contoso", "tenant-id")
}

fn samples(text: &str) -> Vec<&str> {
    // The application health series exist from the start of the process, whatever the scans did
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("esw_process_") && !l.starts_with("esw_build_"))
        .collect()
}

#[test]
fn nothing_is_reported_before_the_first_scan() {
    assert!(samples(&metrics().render_prometheus()).is_empty());
}

#[test]
fn success_publishes_every_series() {
    let m = metrics();
    m.record_success(&scan(vec![credential(-2), credential(8)]));
    let text = m.render_prometheus();
    let lines = samples(&text);

    assert!(lines.contains(&r#"esw_credential_days_left{tenant_name="Contoso",tenant_id="tenant-id",app_name="My App",app_id="app-id",key_id="key-id",type="secret",credential_name="prod-secret"} -2"#));
    assert!(lines.contains(&r#"esw_credentials_expired{tenant_name="Contoso",tenant_id="tenant-id"} 1"#));
    assert!(lines.contains(&r#"esw_credentials_expiring_soon{tenant_name="Contoso",tenant_id="tenant-id"} 1"#));
    assert!(lines.contains(&r#"esw_apps_scanned{tenant_name="Contoso",tenant_id="tenant-id"} 12"#));
    assert!(
        lines.contains(&r#"esw_last_scan_timestamp_seconds{tenant_name="Contoso",tenant_id="tenant-id"} 1792206000"#)
    );
    assert!(lines.contains(&r#"esw_last_scan_success{tenant_name="Contoso",tenant_id="tenant-id"} 1"#));
    assert!(text.contains("# TYPE esw_apps_scanned gauge"));
}

#[test]
fn a_new_scan_drops_the_series_of_the_previous_one() {
    let m = metrics();
    m.record_success(&scan(vec![credential(5)]));
    m.record_success(&scan(vec![]));
    let text = m.render_prometheus();
    assert!(!text.contains("esw_credential_days_left{"));
    assert!(text.contains("esw_credentials_expiring_soon{tenant_name=\"Contoso\",tenant_id=\"tenant-id\"} 0"));
}

#[test]
fn failure_before_any_success_only_reports_timestamp_and_flag() {
    let m = metrics();
    m.record_failure();
    let text = m.render_prometheus();
    let lines = samples(&text);

    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|l| l.starts_with("esw_last_scan_timestamp_seconds{")));
    assert!(lines.contains(&r#"esw_last_scan_success{tenant_name="Contoso",tenant_id="tenant-id"} 0"#));
}

#[test]
fn failure_keeps_the_figures_of_the_last_success() {
    let m = metrics();
    m.record_success(&scan(vec![credential(5)]));
    m.record_failure();
    let text = m.render_prometheus();

    assert!(text.contains("esw_credential_days_left{"));
    assert!(text.contains("esw_apps_scanned{tenant_name=\"Contoso\",tenant_id=\"tenant-id\"} 12"));
    assert!(text.contains("esw_last_scan_success{tenant_name=\"Contoso\",tenant_id=\"tenant-id\"} 0"));
}

#[test]
fn label_values_are_escaped() {
    let m = metrics();
    let mut cred = credential(3);
    cred.credential_name = "a\"b\\c\nd".into();
    m.record_success(&scan(vec![cred]));
    assert!(m.render_prometheus().contains(r#"credential_name="a\"b\\c\nd""#));
}

#[test]
fn counters_are_labelled_and_accumulate() {
    let m = metrics();
    m.count_scan(true);
    m.count_scan(true);
    m.count_scan(false);
    m.count_notification("Webhook", false);
    m.count_graph_request(true);
    let text = m.render_prometheus();
    let lines = samples(&text);

    assert!(lines.contains(&r#"esw_scans_total{tenant_name="Contoso",tenant_id="tenant-id",result="success"} 2"#));
    assert!(lines.contains(&r#"esw_scans_total{tenant_name="Contoso",tenant_id="tenant-id",result="failure"} 1"#));
    assert!(lines.contains(
        &r#"esw_notifications_total{tenant_name="Contoso",tenant_id="tenant-id",channel="Webhook",result="failure"} 1"#
    ));
    assert!(
        lines.contains(&r#"esw_graph_requests_total{tenant_name="Contoso",tenant_id="tenant-id",result="success"} 1"#)
    );
    assert!(text.contains("# TYPE esw_scans_total counter"));
}

#[test]
fn scan_duration_is_reported_in_seconds() {
    let m = metrics();
    assert!(!m.render_prometheus().contains("esw_last_scan_duration_seconds{"));

    m.record_scan_duration(std::time::Duration::from_millis(1500));
    assert!(
        samples(&m.render_prometheus())
            .contains(&r#"esw_last_scan_duration_seconds{tenant_name="Contoso",tenant_id="tenant-id"} 1.5"#)
    );
}

#[test]
fn process_start_and_version_are_always_reported() {
    let text = metrics().render_prometheus();
    assert!(text.contains(r#"esw_process_start_time_seconds{tenant_name="Contoso",tenant_id="tenant-id"} "#));
    assert!(text.contains(&format!(
        r#"esw_build_info{{tenant_name="Contoso",tenant_id="tenant-id",version="{}"}} 1"#,
        env!("CARGO_PKG_VERSION")
    )));
}
