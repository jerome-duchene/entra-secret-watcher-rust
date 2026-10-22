# 🔐 Entra Secret Watcher (Rust)

[![Rust](https://img.shields.io/badge/Rust-2024-orange)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-ready-blue)](https://www.docker.com/)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue)](LICENSE)
[![CI](https://github.com/jerome-duchene/entra-secret-watcher-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/jerome-duchene/entra-secret-watcher-rust/actions/workflows/ci.yml)

A Rust rewrite of [entra-secret-watcher](https://github.com/jerome-duchene/entra-secret-watcher) (.NET): same configuration, endpoints, metrics and notification formats, in a ~18 MB image.

A lightweight, self-hosted Docker container that monitors **Microsoft Entra ID** (Azure AD) app registration **secrets and certificates** for expiration, and sends proactive notifications before they expire.

**One container per tenant** — deploy as many as you need, configure everything via environment variables.

## ✨ Features

- 🔍 Scans all app registrations for expiring **client secrets** and **certificates**
- 📊 Grouped report — single notification summarizing all expiring credentials
- 🔔 Multiple notification channels:
  - **Gotify** — self-hosted push notifications
  - **Email** — via Microsoft Graph API (no SMTP needed)
  - **Microsoft Teams** — via Graph API with Adaptive Cards
  - **Generic webhook** — one JSON POST per credential (Jira Automation, n8n, Power Automate…)
- ⏰ Scheduled scanning with a **cron expression** (lightweight: a single static binary, no job framework)
- 🧪 **Dry-run mode** — test without sending notifications
- 📈 **Prometheus metrics** on `/metrics` (pull) — Grafana dashboards and alerting
- 📡 **OpenTelemetry (OTLP push)** of traces, metrics and logs — to Grafana Alloy, an OpenTelemetry Collector, Tempo, Loki…
- 🐳 Tiny Alpine-based Docker image (~18 MB, single static binary)
- 💚 Built-in health check endpoint

## 📋 Prerequisites

### Entra ID App Registration

Create an app registration in each tenant you want to monitor:

1. Go to **Entra ID** → **App registrations** → **New registration**
2. Name: `entra-secret-watcher` (or your preference)
3. Supported account types: **Single tenant**
4. No redirect URI needed

#### Required API Permissions

| Notification Channel | Permission          | Type        |
|---------------------|---------------------|-------------|
| **All**             | `Application.Read.All` | Application |
| **Email**           | `Mail.Send`            | Application |

> **Teams** uses an Incoming Webhook — no additional API permission required.

> **Grant admin consent** after adding the permissions.

#### Create a Client Secret

Add a client secret (or certificate) and note the value — you'll need it for configuration.

> 💡 **Pro tip**: This app registration's secret will also be monitored by the watcher itself! The watcher watches its own credentials.

## 🚀 Quick Start

### 1. Pull the image (or build locally)

```bash
# Pull from GitHub Container Registry
docker pull ghcr.io/jerome-duchene/entra-secret-watcher-rust:latest

# Or build locally
docker build -t entra-secret-watcher-rust:latest .
```

### 2. Run with Docker

```bash
docker run -d \
  --name esw-contoso \
  --restart unless-stopped \
  -p 8080:8080 \
  -e Entra__TenantId="your-tenant-id" \
  -e Entra__ClientId="your-client-id" \
  -e Entra__ClientSecret="your-secret" \
  -e Entra__TenantName="Contoso" \
  -e Notification__Gotify__Enabled="true" \
  -e Notification__Gotify__Url="https://gotify.example.com" \
  -e Notification__Gotify__Token="your-gotify-token" \
  entra-secret-watcher-rust:latest
```

### 3. Verify

```bash
# Health check
curl http://localhost:8080/health
```

## ⚙️ Configuration

All settings are configured via **environment variables** using the `Section__Key` (double underscore) convention. Names are case-insensitive and may be prefixed with `ESW_` (the prefixed variable wins). The configuration is validated at startup: every problem is reported at once and the process exits with code 1.

### Entra ID Connection

| Variable | Description | Required |
|----------|-------------|----------|
| `Entra__TenantId` | Azure AD tenant ID | ✅ |
| `Entra__ClientId` | App registration client ID | ✅ |
| `Entra__ClientSecret` | App registration client secret | ✅ |
| `Entra__TenantName` | Friendly name (used in notifications) | No (default: `Default`) |

### Server

| Variable | Description | Default |
|----------|-------------|---------|
| `Server__Listen` | Address of the HTTP server (`/health`, `/metrics`, `/scan`) | `0.0.0.0:8080` |

### Watcher Settings

| Variable | Description | Default |
|----------|-------------|---------|
| `Watcher__ThresholdDays` | Days before expiration to alert | `30` |
| `Watcher__CronSchedule` | Cron expression (5 fields, UTC) | `0 8 * * *` (daily 8AM) |
| `Watcher__DryRun` | Log results without sending notifications | `false` |
| `Watcher__GroupedReport` | Send one grouped notification | `true` |
| `Watcher__RunOnStartup` | Also run a scan as soon as the app starts (metrics are available right after a restart) | `false` |
| `Watcher__ManualTriggerSecret` | Enables `POST /scan` (see [Trigger a scan on demand](#-trigger-a-scan-on-demand)). At least 16 characters; empty disables the endpoint | *(empty)* |

### Notification — Gotify

| Variable | Description |
|----------|-------------|
| `Notification__Gotify__Enabled` | `true` / `false` |
| `Notification__Gotify__Url` | Gotify server URL |
| `Notification__Gotify__Token` | Gotify application token |

### Notification — Email (via Graph API)

| Variable | Description |
|----------|-------------|
| `Notification__Email__Enabled` | `true` / `false` |
| `Notification__Email__From` | Sender email (must be a valid mailbox or shared mailbox) |
| `Notification__Email__To` | Comma-separated recipient addresses |

> Requires `Mail.Send` application permission on the app registration.

### Notification — Teams (via Incoming Webhook)

| Variable | Description |
|----------|-------------|
| `Notification__Teams__Enabled` | `true` / `false` |
| `Notification__Teams__WebhookUrl` | Incoming Webhook URL from the Teams channel connector |

> No Graph API permission required. Create the webhook in Teams under **Channel → Connectors → Incoming Webhook**.

### Notification — Generic webhook

| Variable | Description | Default |
|----------|-------------|---------|
| `Notification__Webhook__Enabled` | `true` / `false` | `false` |
| `Notification__Webhook__Url` | URL receiving the POST requests | |
| `Notification__Webhook__Secret` | Optional shared secret sent in a header | |
| `Notification__Webhook__SecretHeaderName` | Header carrying the secret | `X-Webhook-Token` |

One request is sent **per expiring credential**, regardless of `Watcher__GroupedReport`:

```json
{
  "event": "credential.expiring",
  "tenantName": "Contoso",
  "tenantId": "…",
  "scannedAt": "2026-10-17T03:00:00Z",
  "credential": {
    "keyId": "3f1c…",
    "appId": "…",
    "appName": "My App",
    "type": "secret",
    "name": "prod-secret",
    "expiresOn": "2026-10-25T00:00:00Z",
    "daysLeft": 8,
    "status": "expiringSoon"
  }
}
```

`credential.keyId` is the Entra key identifier: it uniquely identifies a secret or certificate
(names can be empty or duplicated). The tool is stateless and calls the webhook at every scan,
so the receiver is responsible for deduplication (e.g. a Jira Automation rule that looks for an
existing issue labelled with the `keyId` before creating one).

### Prometheus metrics

Metrics are always exposed on `GET /metrics` (no configuration needed) for Prometheus to **scrape**.
They can also be **pushed** over OTLP, see [OpenTelemetry](#opentelemetry-otlp-push). Scans run on a schedule
(daily by default) while Prometheus scrapes continuously, so the gauges keep the values of the
**last scan**. They are empty after a container restart until the next scan.

| Metric | Description |
|--------|-------------|
| `esw_credential_days_left{app_name,app_id,key_id,type,credential_name}` | Days left before expiration, one series per credential reported by the last scan (negative = expired). Only credentials within `Watcher__ThresholdDays` are reported. |
| `esw_credentials_expired` | Number of expired credentials found by the last scan |
| `esw_credentials_expiring_soon` | Number of credentials about to expire found by the last scan |
| `esw_apps_scanned` | Number of app registrations inspected by the last scan |
| `esw_last_scan_timestamp_seconds` | Unix timestamp of the last scan attempt |
| `esw_last_scan_success` | `1` if the last scan succeeded, `0` otherwise |

**Every `esw_*` series carries the labels `tenant_name` and `tenant_id`** (from `Entra__TenantName` and
`Entra__TenantId`). With one container per tenant, this is what lets you filter dashboards and route
alerts per tenant, whether Prometheus scrapes `/metrics` or the metrics are pushed over OTLP.

> ⚠️ Set a **distinct `Entra__TenantName`** on each container: its default value is `Default`.
> `tenant_id` always tells tenants apart, but the name is what you will want to read in alerts.

Prometheus scrape configuration (one target per tenant):

```yaml
scrape_configs:
  - job_name: entra-secret-watcher
    scrape_interval: 1m
    static_configs:
      - targets: ["esw-contoso:8080", "esw-fabrikam:8080", "esw-northwind:8080"]
```

The `instance` label identifies the container; `tenant_name` identifies the tenant.

Example alert rules:

```yaml
groups:
  - name: entra-secret-watcher
    rules:
      - alert: EntraCredentialExpiringSoon
        expr: esw_credential_days_left >= 0 and esw_credential_days_left <= 14
        labels: { severity: warning }
        annotations:
          summary: "[{{ $labels.tenant_name }}] {{ $labels.type }} '{{ $labels.credential_name }}' of {{ $labels.app_name }} expires in {{ $value }} day(s)"

      - alert: EntraCredentialExpired
        expr: esw_credential_days_left < 0
        labels: { severity: critical }

      - alert: EntraSecretWatcherScanMissing
        expr: time() - esw_last_scan_timestamp_seconds > 2 * 86400
        labels: { severity: warning }
        annotations:
          summary: "[{{ $labels.tenant_name }}] No scan for more than 2 days"

      - alert: EntraSecretWatcherScanFailed
        expr: esw_last_scan_success == 0
        for: 5m
        labels: { severity: warning }
```

A ready-to-use **Grafana dashboard** (all tenants in one dashboard, filterable per tenant) is available in
[`grafana/`](grafana/README.md).

> **One ticket per credential instead of a daily notification?** Prometheus Alertmanager
> deduplicates alerts and can open a ticket once (e.g. with its Jira receiver). Alternatively use the
> [generic webhook](#notification--generic-webhook) and deduplicate on `credential.keyId` in the receiver.

### OpenTelemetry (OTLP push)

Disabled by default. When enabled, the app **pushes** its telemetry over OTLP, in addition to the
`/metrics` endpoint (which stays available for Prometheus scraping):

| Signal | Content |
|--------|---------|
| **Traces** | Scan job and Graph scan |
| **Metrics** | The `esw_*` metrics above |
| **Logs** | Application logs |

| Variable | Description | Default |
|----------|-------------|---------|
| `OpenTelemetry__Enabled` | Enable the OTLP export | `false` |
| `OpenTelemetry__Endpoint` | OTLP endpoint. With `HttpProtobuf`, give the base URL (`/v1/traces`, `/v1/metrics`, `/v1/logs` are appended). When empty, the standard `OTEL_EXPORTER_OTLP_*` variables apply | `http://localhost:4317` (gRPC) |
| `OpenTelemetry__Protocol` | `Grpc` or `HttpProtobuf` | `Grpc` |
| `OpenTelemetry__ServiceName` | `service.name` resource attribute | `entra-secret-watcher` |

Every signal carries the resource attributes `service.name`, `service.instance.id` (the tenant id, so
each container is a distinct instance), `tenant.name` and `tenant.id`. The `esw_*` metrics also carry
`tenant_name` and `tenant_id` as regular labels, so they identify the tenant without any extra
Alloy configuration. Extra resource attributes can be added with the standard
`OTEL_RESOURCE_ATTRIBUTES` variable (e.g. `OTEL_RESOURCE_ATTRIBUTES=deployment.environment=prod`).

#### Example with Grafana Alloy

The application pushes to Alloy, which forwards to Prometheus (metrics), Tempo (traces) and Loki (logs):

```alloy
otelcol.receiver.otlp "default" {
  grpc { endpoint = "0.0.0.0:4317" }
  http { endpoint = "0.0.0.0:4318" }

  output {
    metrics = [otelcol.processor.batch.default.input]
    traces  = [otelcol.processor.batch.default.input]
    logs    = [otelcol.processor.batch.default.input]
  }
}

otelcol.processor.batch "default" {
  output {
    metrics = [otelcol.exporter.prometheus.default.input]
    traces  = [otelcol.exporter.otlp.tempo.input]
    logs    = [otelcol.exporter.loki.default.input]
  }
}

// Metrics → Prometheus (remote write)
otelcol.exporter.prometheus "default" {
  // Optional: also turn resource attributes (tenant.name, tenant.id, …) into labels on every series
  // resource_to_telemetry_conversion = true
  forward_to = [prometheus.remote_write.default.receiver]
}
prometheus.remote_write "default" {
  endpoint { url = "http://prometheus:9090/api/v1/write" }
}

// Traces → Tempo
otelcol.exporter.otlp "tempo" {
  client { url = "tempo:4317"  tls { insecure = true } }
}

// Logs → Loki
otelcol.exporter.loki "default" {
  forward_to = [loki.write.default.receiver]
}
loki.write "default" {
  endpoint { url = "http://loki:3100/loki/api/v1/push" }
}
```

Application side:

```yaml
environment:
  OpenTelemetry__Enabled: "true"
  OpenTelemetry__Endpoint: "http://alloy:4317"
```

> Prometheus must accept remote write (`--web.enable-remote-write-receiver`).
> Do not configure both a Prometheus scrape **and** the OTLP push for the same instance: the series
> would be duplicated. Pick one path per deployment.

## 🐳 Multi-Tenant Deployment

Use `docker-compose.yml` to deploy one container per tenant:

```yaml
services:
  watcher-contoso:
    image: entra-secret-watcher-rust:latest
    environment:
      Entra__TenantId: "tenant-a-id"
      Entra__TenantName: "Contoso"
      # ... other config

  watcher-fabrikam:
    image: entra-secret-watcher-rust:latest
    environment:
      Entra__TenantId: "tenant-b-id"
      Entra__TenantName: "Fabrikam"
      # ... other config
```

See the included `docker-compose.yml` for a complete example.

## 🔍 Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check (returns tenant name and timestamp) |
| `GET /metrics` | Prometheus metrics (see [Prometheus metrics](#prometheus-metrics)) |
| `POST /scan` | Trigger a scan on demand (disabled unless `Watcher__ManualTriggerSecret` is set) |

## ▶️ Trigger a scan on demand

To run a scan outside the schedule, set a secret and call `POST /scan`:

```bash
-e Watcher__ManualTriggerSecret="a-long-random-secret"   # at least 16 characters
```

```bash
curl -X POST http://localhost:8080/scan -H "X-Scan-Token: a-long-random-secret"
# 202 Accepted
```

- The scan starts in the background and runs like a scheduled one (notifications, metrics, `Watcher__DryRun`…).
  Follow it in the logs. Scans never overlap: a call made during a scan waits for it to finish.
- `401` if the header is missing or wrong. Without `Watcher__ManualTriggerSecret`, the endpoint does not exist.
- Every call starts one more scan: do not expose the endpoint publicly, and combine it with the
  [dry-run mode](#-dry-run-mode) to test without sending notifications.
- `Watcher__RunOnStartup=true` additionally runs a scan at every start of the container.

## 🧪 Dry-Run Mode

Test the scanner without sending any notifications:

```bash
-e Watcher__DryRun="true"
```

All detected credentials will be logged to stdout — useful for validating permissions and connectivity before going live.

## 🏗️ Architecture

```
┌─────────────────────────────────────────────┐
│          entra-secret-watcher               │
│                                             │
│  ┌───────────┐    ┌──────────────────────┐  │
│  │ Cron       │───▶│ CredentialScanJob    │  │
│  │ Scheduler  │    │                      │  │
│  └───────────┘    │  ┌────────────────┐  │  │
│                   │  │ Graph API Scan │  │  │
│  ┌───────────┐    │  └───────┬────────┘  │  │
│  │ /health   │    │          │           │  │
│  │           │    │  ┌───────▼────────┐  │  │
│  └───────────┘    │  │ Notification   │  │  │
│                   │  │ Dispatcher     │  │  │
│                   │  └───┬───┬───┬────┘  │  │
│                   └──────┼───┼───┼───────┘  │
│                          │   │   │          │
└──────────────────────────┼───┼───┼──────────┘
                           │   │   │
                    ┌──────┘   │   └──────┐
                    ▼          ▼          ▼
                 Gotify    Email      Teams
                          (Graph)   (Webhook)

   Also: generic webhook (one POST per credential)
         /metrics scraped by Prometheus
```

## 🧪 Development

### Running tests

```bash
cargo test
```

All external services (Entra ID token endpoint, Microsoft Graph, Gotify, Teams, generic webhook) are
replaced by local mock HTTP servers, so the tests need no credentials and no network.

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

> On Windows the MSVC target needs the *MSVC x64/x86 build tools* and a Windows SDK. Without them you can
> run everything in a container: `docker run --rm -v "$PWD:/work" -w /work rust:latest cargo test`.
### CI/CD

The project uses GitHub Actions (`.github/workflows/ci.yml`):

- **Every push / PR** → format check, clippy and tests run automatically
- **On `v*.*.*` tag** → checks + multi-platform Docker image (`linux/amd64`, `linux/arm64`) published to `ghcr.io`

To publish a release:

1. Move the `[Unreleased]` entries of [CHANGELOG.md](CHANGELOG.md) under a new version heading
   (e.g. `## [0.5.0] - 2026-12-01`) and commit.
2. Tag and push:

```bash
git tag v0.5.0
git push origin v0.5.0
```

The tag triggers the Docker image publication and creates a **GitHub Release** whose notes are
taken from the matching CHANGELOG section (falling back to auto-generated notes, grouped by
the labels configured in `.github/release.yml`). See [CHANGELOG.md](CHANGELOG.md) for the history.

## 🦀 Differences with the .NET version

The configuration, endpoints, metric names/labels, webhook payload and notification content are
identical, so an existing deployment (compose file, Prometheus rules, Grafana dashboard) can switch
image without changes. What differs:

| Area | .NET version | Rust version |
|------|--------------|--------------|
| Listen address | `ASPNETCORE_URLS` / Kestrel | `Server__Listen` (default `0.0.0.0:8080`) |
| Gotify token | Query string (`/message?token=…`) | `X-Gotify-Key` header, so it never appears in a logged URL |
| Graph token | Requested on every call | Cached until one minute before expiry |
| HTML email | App and credential names inserted as is | HTML-escaped |
| OTLP traces / metrics | Also ASP.NET Core and HttpClient instrumentation | Scan job and Graph scan spans, and the `esw_*` metrics only |
| Invalid configuration | Startup failure (exception) | All errors listed, exit code 1 |
## 📄 License

Apache 2.0 — see [LICENSE](LICENSE).

## 🤝 Contributing

Contributions welcome! Please open an issue or submit a pull request.
