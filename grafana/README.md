# Grafana dashboards

Two ready-to-use dashboards, to import independently:

| Dashboard | File | Answers |
|-----------|------|---------|
| **Entra Secret Watcher** | [`entra-secret-watcher.json`](entra-secret-watcher.json) | What expires, and when? (the business value) |
| **Entra Secret Watcher - Application** | [`entra-secret-watcher-app.json`](entra-secret-watcher-app.json) | Is the watcher itself healthy? (see [below](#application-dashboard)) |

The first one covers the metrics,
logs and traces of Entra Secret Watcher, whether they reach Prometheus through
[Grafana Alloy (OTLP push)](../README.md#opentelemetry-otlp-push) or through a
[Prometheus scrape of `/metrics`](../README.md#prometheus-metrics).

## One dashboard for all your tenants

You do **not** need one dashboard per tenant. The **Tenant** variable lists every `tenant_name` found in
Prometheus (multi-select, *All* by default): the *Tenants* table shows one row per tenant, and picking a
tenant filters every panel. Just make sure each container has a distinct `Entra__TenantName`.

## Panels

| Row | Panel | Source |
|-----|-------|--------|
| Overview | Expired credentials, Expiring soon, Apps scanned, Last scan (OK / FAILED), Time since last scan | Prometheus |
| Per tenant | One row per tenant: last scan status and age, expired, expiring soon, apps scanned | Prometheus |
| Credentials to renew | Table of the credentials reported by the last scan (tenant, app, type, name, days left, key id…), sorted by days left | Prometheus |
| Credentials to renew | Days left per credential, expired / expiring soon over time | Prometheus |
| Logs (Loki) | Application logs, with a free-text filter | Loki *(optional)* |
| Traces (Tempo) | Recent traces of the scan job and HTTP requests | Tempo *(optional)* |

Only the Prometheus datasource is required. If you do not use Loki or Tempo, delete the last two rows
(or ignore them).

## Import

**From the UI**: *Dashboards → New → Import → Upload JSON file*, then pick your datasources.

**With provisioning** (files):

```yaml
# /etc/grafana/provisioning/dashboards/entra-secret-watcher.yml
apiVersion: 1
providers:
  - name: entra-secret-watcher
    type: file
    options:
      path: /var/lib/grafana/dashboards/entra-secret-watcher
```

and mount [`entra-secret-watcher.json`](entra-secret-watcher.json) in that folder.

The dashboard uses datasource **variables** (`Prometheus`, `Loki`, `Tempo`, at the top of the page), so it
works with any datasource names. It has the fixed uid `entra-secret-watcher`.

## What the queries expect

- Metrics named `esw_*` with the `tenant_name` label (see the [metrics list](../README.md#prometheus-metrics)).
  Alloy keeps these names: the gauges have no unit, so no suffix is added during the OTLP → Prometheus
  conversion.
- **Logs**: the *Logs selector* variable defaults to `{job="entra-secret-watcher"}`. The labels depend on how
  your Alloy pipeline writes to Loki (with `otelcol.exporter.loki`, `job` comes from `service.name`):
  adapt the variable if your streams are labelled differently.
- **Traces**: the query is `{ resource.service.name = "entra-secret-watcher" && resource.tenant.name =~ "$tenant" }`.
  If you changed `OpenTelemetry__ServiceName`, edit the dashboard variable `service_name` (*Settings → Variables*).
- **Thresholds**: *Time since last scan* turns orange after 36 h and red after 48 h, which suits the default
  daily schedule. Adapt them to your `Watcher__CronSchedule`.
- **Days left** colours: red when expired, orange up to 14 days, yellow up to 30, green beyond.

## Application dashboard

[`entra-secret-watcher-app.json`](entra-secret-watcher-app.json) (uid `entra-secret-watcher-app`) looks at the
application, not at the credentials. Same datasource variables and **Tenant** filter as the first dashboard.

| Row | Panel | Source |
|-----|-------|--------|
| Health | Version, Uptime, Restarts, Failed scans, Failed notifications, Graph error rate (over the selected time range) | Prometheus |
| Scans and Microsoft Graph | Scans per result, Scan duration, Graph calls per result | Prometheus |
| Notifications | Attempts per channel and result, totals since the last restart | Prometheus |
| Errors (Loki) | Errors and warnings over time, Log volume by level, Error logs | Loki *(optional)* |
| Traces (Tempo) | Traces in error, Slowest scans (> 10 s) | Tempo *(optional)* |

- The counters (`esw_scans_total`, `esw_notifications_total`, `esw_graph_requests_total`) start at 0 with the
  process and a series only exists after its first event, so panels show nothing until a scan has happened.
- **Errors (Loki)** relies on the `detected_level` that Loki 3 adds to every log line. With an older Loki, or
  streams labelled differently, edit the *Errors pipeline* variable (default `| detected_level="error"`), and
  the two queries of *Errors and warnings*.
- **Memory and CPU** of the container are not exposed by the application: use cAdvisor or your container
  runtime's metrics next to this dashboard.
- Useful alerts: a failed scan (`increase(esw_scans_total{result="failure"}[1d]) > 0`), a failed notification
  (`increase(esw_notifications_total{result="failure"}[1d]) > 0`) and a restart loop
  (`changes(esw_process_start_time_seconds[1h]) > 2`).

## Tested with

The Prometheus panels of the first dashboard were checked against Grafana (latest image at the time of
writing) with a Prometheus scraping sample `esw_*` series for two tenants. The Loki and Tempo panels were not
tested against real data, and the application dashboard has not been imported into Grafana yet: only its
JSON validity and the metrics it relies on (unit tests) were checked.
