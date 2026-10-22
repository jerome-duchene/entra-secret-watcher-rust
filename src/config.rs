//! Typed configuration read from environment variables.
//!
//! Same convention as the .NET version: `Section__Key` (double underscore), case-insensitive,
//! optionally prefixed with `ESW_` (the prefixed variable wins). Everything is validated at
//! startup and all errors are reported at once.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;

use croner::Cron;

const ENV_PREFIX: &str = "ESW_";

pub const DEFAULT_SECRET_HEADER_NAME: &str = "X-Webhook-Token";
pub const DEFAULT_SERVICE_NAME: &str = "entra-secret-watcher";

/// Flat view of the settings: keys are lowercased `section:key` paths.
#[derive(Debug, Default, Clone)]
pub struct Settings(HashMap<String, String>);

impl Settings {
    pub fn from_env() -> Self {
        Self::from_pairs(std::env::vars())
    }

    pub fn from_pairs<I, K, V>(vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<String>,
    {
        let vars: Vec<(String, String)> = vars.into_iter().map(|(k, v)| (k.as_ref().to_string(), v.into())).collect();

        let mut map = HashMap::new();
        // Plain variables first, then ESW_-prefixed ones which override them
        for (name, value) in vars.iter().filter(|(n, _)| !has_prefix(n)) {
            map.insert(normalize(name), value.clone());
        }
        for (name, value) in vars.iter().filter(|(n, _)| has_prefix(n)) {
            map.insert(normalize(&name[ENV_PREFIX.len()..]), value.clone());
        }
        Self(map)
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(&key.to_lowercase()).map(String::as_str)
    }

    fn string(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_string()
    }

    fn bool(&self, key: &str, default: bool, errors: &mut Vec<String>) -> bool {
        match self.get(key).map(str::trim) {
            None | Some("") => default,
            Some(v) if v.eq_ignore_ascii_case("true") => true,
            Some(v) if v.eq_ignore_ascii_case("false") => false,
            Some(v) => {
                errors.push(format!("{key} must be 'true' or 'false' (got '{v}')."));
                default
            }
        }
    }

    fn int(&self, key: &str, default: i64, errors: &mut Vec<String>) -> i64 {
        match self.get(key).map(str::trim) {
            None | Some("") => default,
            Some(v) => v.parse().unwrap_or_else(|_| {
                errors.push(format!("{key} must be an integer (got '{v}')."));
                default
            }),
        }
    }
}

fn has_prefix(name: &str) -> bool {
    name.len() > ENV_PREFIX.len() && name[..ENV_PREFIX.len()].eq_ignore_ascii_case(ENV_PREFIX)
}

fn normalize(name: &str) -> String {
    name.replace("__", ":").to_lowercase()
}

#[derive(Debug, Clone)]
pub struct EntraOptions {
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
    /// Friendly name for this tenant (used in notifications).
    pub tenant_name: String,
}

#[derive(Debug, Clone)]
pub struct WatcherOptions {
    /// Number of days before expiration to trigger a warning.
    pub threshold_days: i64,
    /// Cron expression for the scan schedule (5 fields, UTC).
    pub cron_schedule: String,
    /// Scan and log but don't send notifications.
    pub dry_run: bool,
    /// Send a single grouped notification instead of one per credential.
    pub grouped_report: bool,
    /// Run a scan as soon as the application starts, in addition to the schedule.
    pub run_on_startup: bool,
    /// Enables `POST /scan`; empty disables the endpoint.
    pub manual_trigger_secret: String,
}

impl Default for WatcherOptions {
    fn default() -> Self {
        Self {
            threshold_days: 30,
            cron_schedule: "0 8 * * *".into(),
            dry_run: false,
            grouped_report: true,
            run_on_startup: false,
            manual_trigger_secret: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GotifyOptions {
    pub enabled: bool,
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Default)]
pub struct EmailOptions {
    pub enabled: bool,
    /// Sender address: a valid mailbox or shared mailbox of the tenant (Graph `Mail.Send`).
    pub from: String,
    /// Comma-separated list of recipients.
    pub to: String,
}

#[derive(Debug, Clone, Default)]
pub struct TeamsOptions {
    pub enabled: bool,
    pub webhook_url: String,
}

#[derive(Debug, Clone)]
pub struct WebhookOptions {
    pub enabled: bool,
    pub url: String,
    pub secret: String,
    pub secret_header_name: String,
}

impl Default for WebhookOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            secret: String::new(),
            secret_header_name: DEFAULT_SECRET_HEADER_NAME.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NotificationOptions {
    pub gotify: GotifyOptions,
    pub email: EmailOptions,
    pub teams: TeamsOptions,
    pub webhook: WebhookOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpTransport {
    Grpc,
    HttpProtobuf,
}

#[derive(Debug, Clone)]
pub struct OpenTelemetryOptions {
    /// Push traces, metrics and logs over OTLP.
    pub enabled: bool,
    /// OTLP endpoint. When empty, the standard `OTEL_EXPORTER_OTLP_*` variables apply.
    pub endpoint: String,
    pub protocol: OtlpTransport,
    pub service_name: String,
}

impl Default for OpenTelemetryOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            protocol: OtlpTransport::Grpc,
            service_name: DEFAULT_SERVICE_NAME.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// Address of the HTTP server (`/health`, `/metrics`, `/scan`).
    pub listen: SocketAddr,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { listen: SocketAddr::from(([0, 0, 0, 0], 8080)) }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerOptions,
    pub watcher: WatcherOptions,
    pub entra: EntraOptions,
    pub notification: NotificationOptions,
    pub open_telemetry: OpenTelemetryOptions,
}

/// All the validation errors found in the configuration.
#[derive(Debug)]
pub struct ConfigError(pub Vec<String>);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid configuration:")?;
        for error in &self.0 {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        Self::from_settings(&Settings::from_env())
    }

    pub fn from_settings(s: &Settings) -> Result<Self, ConfigError> {
        let mut errors = Vec::new();

        let server = read_server(s, &mut errors);
        let watcher = read_watcher(s, &mut errors);
        let entra = read_entra(s, &mut errors);
        let notification = read_notification(s, &mut errors);
        let open_telemetry = read_open_telemetry(s, &mut errors);

        if errors.is_empty() {
            Ok(Self { server, watcher, entra, notification, open_telemetry })
        } else {
            Err(ConfigError(errors))
        }
    }
}

fn read_server(s: &Settings, errors: &mut Vec<String>) -> ServerOptions {
    let default = ServerOptions::default();
    match s.get("Server:Listen").map(str::trim) {
        None | Some("") => default,
        Some(v) => v.parse().map(|listen| ServerOptions { listen }).unwrap_or_else(|_| {
            errors.push(format!("Server:Listen must be an address like 0.0.0.0:8080 (got '{v}')."));
            default
        }),
    }
}

fn read_watcher(s: &Settings, errors: &mut Vec<String>) -> WatcherOptions {
    let d = WatcherOptions::default();
    let options = WatcherOptions {
        threshold_days: s.int("Watcher:ThresholdDays", d.threshold_days, errors),
        cron_schedule: s.string("Watcher:CronSchedule", &d.cron_schedule),
        dry_run: s.bool("Watcher:DryRun", d.dry_run, errors),
        grouped_report: s.bool("Watcher:GroupedReport", d.grouped_report, errors),
        run_on_startup: s.bool("Watcher:RunOnStartup", d.run_on_startup, errors),
        manual_trigger_secret: s.string("Watcher:ManualTriggerSecret", ""),
    };

    if !(1..=365).contains(&options.threshold_days) {
        errors.push("ThresholdDays must be between 1 and 365.".into());
    }
    if options.cron_schedule.trim().is_empty() {
        errors.push("CronSchedule is required.".into());
    } else if let Err(e) = options.cron_schedule.parse::<Cron>() {
        errors.push(format!("CronSchedule '{}' is not a valid cron expression: {e}", options.cron_schedule));
    }
    let secret_len = options.manual_trigger_secret.chars().count();
    if secret_len != 0 && secret_len < 16 {
        errors.push("ManualTriggerSecret must be empty (endpoint disabled) or at least 16 characters.".into());
    }
    options
}

fn read_entra(s: &Settings, errors: &mut Vec<String>) -> EntraOptions {
    let options = EntraOptions {
        tenant_id: s.string("Entra:TenantId", ""),
        client_id: s.string("Entra:ClientId", ""),
        client_secret: s.string("Entra:ClientSecret", ""),
        tenant_name: s.string("Entra:TenantName", "Default"),
    };

    for (name, value) in
        [("TenantId", &options.tenant_id), ("ClientId", &options.client_id), ("ClientSecret", &options.client_secret)]
    {
        if value.is_empty() {
            errors.push(format!("Entra:{name} is required."));
        }
    }
    options
}

fn read_notification(s: &Settings, errors: &mut Vec<String>) -> NotificationOptions {
    let webhook_defaults = WebhookOptions::default();
    let options = NotificationOptions {
        gotify: GotifyOptions {
            enabled: s.bool("Notification:Gotify:Enabled", false, errors),
            url: s.string("Notification:Gotify:Url", ""),
            token: s.string("Notification:Gotify:Token", ""),
        },
        email: EmailOptions {
            enabled: s.bool("Notification:Email:Enabled", false, errors),
            from: s.string("Notification:Email:From", ""),
            to: s.string("Notification:Email:To", ""),
        },
        teams: TeamsOptions {
            enabled: s.bool("Notification:Teams:Enabled", false, errors),
            webhook_url: s.string("Notification:Teams:WebhookUrl", ""),
        },
        webhook: WebhookOptions {
            enabled: s.bool("Notification:Webhook:Enabled", false, errors),
            url: s.string("Notification:Webhook:Url", ""),
            secret: s.string("Notification:Webhook:Secret", ""),
            secret_header_name: s.string("Notification:Webhook:SecretHeaderName", &webhook_defaults.secret_header_name),
        },
    };

    let w = &options.webhook;
    if w.enabled {
        check_http_url(&w.url, "Webhook:Url", "Webhook", errors);
        if !w.secret.is_empty() && w.secret_header_name.trim().is_empty() {
            errors.push("Webhook:SecretHeaderName is required when Webhook:Secret is set.".into());
        }
    }

    let g = &options.gotify;
    if g.enabled {
        check_http_url(&g.url, "Gotify:Url", "Gotify", errors);
        if g.token.trim().is_empty() {
            errors.push("Gotify:Token is required when Gotify is enabled.".into());
        }
    }

    let e = &options.email;
    if e.enabled {
        if e.from.trim().is_empty() {
            errors.push("Email:From is required when Email is enabled.".into());
        } else if !is_valid_email(&e.from) {
            errors.push("Email:From must be a valid email address.".into());
        }
        if e.to.trim().is_empty() {
            errors.push("Email:To is required when Email is enabled.".into());
        }
    }

    let t = &options.teams;
    if t.enabled {
        check_http_url(&t.webhook_url, "Teams:WebhookUrl", "Teams", errors);
    }

    options
}

fn read_open_telemetry(s: &Settings, errors: &mut Vec<String>) -> OpenTelemetryOptions {
    let d = OpenTelemetryOptions::default();

    let protocol = match s.get("OpenTelemetry:Protocol").map(str::trim) {
        None | Some("") => d.protocol,
        Some(v) if v.eq_ignore_ascii_case("grpc") => OtlpTransport::Grpc,
        Some(v) if v.eq_ignore_ascii_case("httpprotobuf") => OtlpTransport::HttpProtobuf,
        Some(v) => {
            errors.push(format!("OpenTelemetry:Protocol must be 'Grpc' or 'HttpProtobuf' (got '{v}')."));
            d.protocol
        }
    };

    let options = OpenTelemetryOptions {
        enabled: s.bool("OpenTelemetry:Enabled", false, errors),
        endpoint: s.string("OpenTelemetry:Endpoint", ""),
        protocol,
        service_name: s.string("OpenTelemetry:ServiceName", &d.service_name),
    };

    if options.enabled {
        if !options.endpoint.trim().is_empty() && !is_http_url(&options.endpoint) {
            errors.push("OpenTelemetry:Endpoint must be a valid HTTP(S) URL.".into());
        }
        if options.service_name.trim().is_empty() {
            errors.push("OpenTelemetry:ServiceName must not be empty.".into());
        }
    }
    options
}

/// `key` is the config key shown in messages, `name` the feature name ("Gotify is enabled").
fn check_http_url(value: &str, key: &str, name: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{key} is required when {name} is enabled."));
    } else if !is_http_url(value) {
        errors.push(format!("{key} must be a valid HTTP(S) URL."));
    }
}

fn is_http_url(value: &str) -> bool {
    reqwest::Url::parse(value).map(|u| matches!(u.scheme(), "http" | "https") && u.has_host()).unwrap_or(false)
}

/// Mirrors .NET's `EmailAddressAttribute`: a single `@`, not first nor last.
fn is_valid_email(value: &str) -> bool {
    match (value.find('@'), value.rfind('@')) {
        (Some(first), Some(last)) => first == last && first > 0 && first < value.len() - 1,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
