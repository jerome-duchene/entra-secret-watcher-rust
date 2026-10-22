# Grafana dashboard

[`entra-secret-watcher.json`](entra-secret-watcher.json) is a ready-to-use dashboard for the metrics,
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

## Tested with

The Prometheus panels were checked against Grafana (latest image at the time of writing) with a
Prometheus scraping sample `esw_*` series for two tenants. The Loki and Tempo panels were not tested
against real data.
