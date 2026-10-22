use super::*;

fn settings(pairs: &[(&str, &str)]) -> Settings {
    Settings::from_pairs(pairs.iter().copied())
}

fn base() -> Vec<(&'static str, &'static str)> {
    vec![("Entra__TenantId", "tenant"), ("Entra__ClientId", "client"), ("Entra__ClientSecret", "secret")]
}

fn errors_for(extra: &[(&str, &str)]) -> Vec<String> {
    let mut pairs = base();
    pairs.extend_from_slice(extra);
    Config::from_settings(&settings(&pairs)).unwrap_err().0
}

#[test]
fn minimal_configuration_uses_defaults() {
    let config = Config::from_settings(&settings(&base())).unwrap();
    assert_eq!(config.entra.tenant_name, "Default");
    assert_eq!(config.watcher.threshold_days, 30);
    assert_eq!(config.watcher.cron_schedule, "0 8 * * *");
    assert!(config.watcher.grouped_report);
    assert!(!config.watcher.dry_run);
    assert!(!config.open_telemetry.enabled);
    assert_eq!(config.notification.webhook.secret_header_name, "X-Webhook-Token");
}

#[test]
fn listen_address_defaults_to_8080_and_can_be_overridden() {
    let config = Config::from_settings(&settings(&base())).unwrap();
    assert_eq!(config.server.listen.to_string(), "0.0.0.0:8080");

    let mut pairs = base();
    pairs.push(("Server__Listen", "127.0.0.1:9000"));
    let config = Config::from_settings(&settings(&pairs)).unwrap();
    assert_eq!(config.server.listen.to_string(), "127.0.0.1:9000");

    assert_eq!(errors_for(&[("Server__Listen", "localhost")]).len(), 1);
}

#[test]
fn keys_are_case_insensitive_and_esw_prefix_wins() {
    let config = Config::from_settings(&settings(&[
        ("entra__tenantid", "t"),
        ("ENTRA__CLIENTID", "c"),
        ("Entra__ClientSecret", "plain"),
        ("ESW_Entra__ClientSecret", "prefixed"),
    ]))
    .unwrap();
    assert_eq!(config.entra.tenant_id, "t");
    assert_eq!(config.entra.client_id, "c");
    assert_eq!(config.entra.client_secret, "prefixed");
}

#[test]
fn missing_entra_settings_are_all_reported() {
    let errors = Config::from_settings(&settings(&[])).unwrap_err().0;
    assert!(errors.contains(&"Entra:TenantId is required.".to_string()));
    assert!(errors.contains(&"Entra:ClientId is required.".to_string()));
    assert!(errors.contains(&"Entra:ClientSecret is required.".to_string()));
}

#[test]
fn threshold_must_be_in_range() {
    for value in ["0", "366", "-4"] {
        let errors = errors_for(&[("Watcher__ThresholdDays", value)]);
        assert_eq!(errors, vec!["ThresholdDays must be between 1 and 365."], "value {value}");
    }
}

#[test]
fn invalid_cron_is_rejected() {
    let errors = errors_for(&[("Watcher__CronSchedule", "not a cron")]);
    assert!(errors[0].starts_with("CronSchedule 'not a cron' is not a valid cron expression"));
}

#[test]
fn manual_trigger_secret_must_be_empty_or_long_enough() {
    assert!(!errors_for(&[("Watcher__ManualTriggerSecret", "short")]).is_empty());
    let mut pairs = base();
    pairs.push(("Watcher__ManualTriggerSecret", "0123456789abcdef"));
    assert!(Config::from_settings(&settings(&pairs)).is_ok());
}

#[test]
fn unparsable_values_are_reported() {
    let errors = errors_for(&[("Watcher__DryRun", "maybe"), ("Watcher__ThresholdDays", "abc")]);
    assert_eq!(errors.len(), 2);
}

#[test]
fn disabled_channels_are_not_validated() {
    let mut pairs = base();
    pairs.push(("Notification__Gotify__Url", "garbage"));
    assert!(Config::from_settings(&settings(&pairs)).is_ok());
}

#[test]
fn enabled_gotify_requires_valid_url_and_token() {
    let errors = errors_for(&[("Notification__Gotify__Enabled", "true")]);
    assert_eq!(
        errors,
        vec!["Gotify:Url is required when Gotify is enabled.", "Gotify:Token is required when Gotify is enabled."]
    );
    let errors = errors_for(&[
        ("Notification__Gotify__Enabled", "true"),
        ("Notification__Gotify__Url", "ftp://x"),
        ("Notification__Gotify__Token", "t"),
    ]);
    assert_eq!(errors, vec!["Gotify:Url must be a valid HTTP(S) URL."]);
}

#[test]
fn enabled_email_requires_valid_sender_and_recipients() {
    let errors = errors_for(&[("Notification__Email__Enabled", "true")]);
    assert_eq!(
        errors,
        vec!["Email:From is required when Email is enabled.", "Email:To is required when Email is enabled."]
    );
    let errors = errors_for(&[
        ("Notification__Email__Enabled", "true"),
        ("Notification__Email__From", "not-an-email"),
        ("Notification__Email__To", "a@b.c"),
    ]);
    assert_eq!(errors, vec!["Email:From must be a valid email address."]);
}

#[test]
fn enabled_teams_requires_http_webhook_url() {
    let errors = errors_for(&[("Notification__Teams__Enabled", "true")]);
    assert_eq!(errors, vec!["Teams:WebhookUrl is required when Teams is enabled."]);
    let errors = errors_for(&[("Notification__Teams__Enabled", "true"), ("Notification__Teams__WebhookUrl", "nope")]);
    assert_eq!(errors, vec!["Teams:WebhookUrl must be a valid HTTP(S) URL."]);
}

#[test]
fn enabled_webhook_validates_url_and_secret_header() {
    let errors = errors_for(&[("Notification__Webhook__Enabled", "true")]);
    assert_eq!(errors, vec!["Webhook:Url is required when Webhook is enabled."]);
    let errors = errors_for(&[
        ("Notification__Webhook__Enabled", "true"),
        ("Notification__Webhook__Url", "https://hook.example.com"),
        ("Notification__Webhook__Secret", "s"),
        ("Notification__Webhook__SecretHeaderName", " "),
    ]);
    assert_eq!(errors, vec!["Webhook:SecretHeaderName is required when Webhook:Secret is set."]);
}

#[test]
fn open_telemetry_is_validated_only_when_enabled() {
    let mut pairs = base();
    pairs.push(("OpenTelemetry__Endpoint", "garbage"));
    assert!(Config::from_settings(&settings(&pairs)).is_ok());

    let errors = errors_for(&[("OpenTelemetry__Enabled", "true"), ("OpenTelemetry__Endpoint", "garbage")]);
    assert_eq!(errors, vec!["OpenTelemetry:Endpoint must be a valid HTTP(S) URL."]);

    let errors = errors_for(&[("OpenTelemetry__Enabled", "true"), ("OpenTelemetry__ServiceName", " ")]);
    assert_eq!(errors, vec!["OpenTelemetry:ServiceName must not be empty."]);
}

#[test]
fn open_telemetry_protocol_is_parsed() {
    let mut pairs = base();
    pairs.push(("OpenTelemetry__Protocol", "HttpProtobuf"));
    let config = Config::from_settings(&settings(&pairs)).unwrap();
    assert_eq!(config.open_telemetry.protocol, OtlpTransport::HttpProtobuf);

    let errors = errors_for(&[("OpenTelemetry__Protocol", "carrier-pigeon")]);
    assert_eq!(errors.len(), 1);
}
