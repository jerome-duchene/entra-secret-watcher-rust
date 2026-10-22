//! Logging, and (optionally) OTLP push of traces, metrics and logs.
//!
//! Console logging and the Prometheus `/metrics` endpoint are always available. When
//! `OpenTelemetry:Enabled` is true, traces, logs and the `esw_*` metrics are also pushed over OTLP.

use std::sync::Arc;

use anyhow::{Context, Result};
use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::config::{Config, OpenTelemetryOptions, OtlpTransport};
use crate::metrics::{self, ScanMetrics};

const METER_NAME: &str = "EntraSecretWatcher";
const DEFAULT_FILTER: &str = "info,hyper=warn";
/// What must never be exported as logs, otherwise the exporter's own activity feeds itself.
const OTLP_LOG_FILTER: &str = "info,hyper=off,hyper_util=off,reqwest=off,tonic=off,h2=off,tower=off,opentelemetry=off";

/// Flushes and stops the OTLP exporters when dropped.
pub struct TelemetryGuard {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
    logger: Option<SdkLoggerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        // Errors here can only be reported on stderr: the log pipeline is going away
        if let Some(tracer) = &self.tracer {
            let _ = tracer.shutdown();
        }
        if let Some(meter) = &self.meter {
            let _ = meter.shutdown();
        }
        if let Some(logger) = &self.logger {
            let _ = logger.shutdown();
        }
    }
}

pub fn init(config: &Config, metrics: Arc<ScanMetrics>) -> Result<TelemetryGuard> {
    let console_filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let console = tracing_subscriber::fmt::layer().with_target(false).with_filter(console_filter());

    let otel = &config.open_telemetry;
    if !otel.enabled {
        tracing_subscriber::registry().with(console).try_init()?;
        return Ok(TelemetryGuard { tracer: None, meter: None, logger: None });
    }

    let resource = resource(otel, &config.entra.tenant_name, &config.entra.tenant_id);

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter(otel).context("Cannot create the OTLP trace exporter")?)
        .build();
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_periodic_exporter(metric_exporter(otel).context("Cannot create the OTLP metric exporter")?)
        .build();
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter(otel).context("Cannot create the OTLP log exporter")?)
        .build();

    register_gauges(&meter_provider, metrics);

    let traces =
        tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer(METER_NAME)).with_filter(console_filter());
    let logs = OpenTelemetryTracingBridge::new(&logger_provider).with_filter(EnvFilter::new(OTLP_LOG_FILTER));

    tracing_subscriber::registry().with(console).with(traces).with(logs).try_init()?;

    Ok(TelemetryGuard { tracer: Some(tracer_provider), meter: Some(meter_provider), logger: Some(logger_provider) })
}

/// One container per tenant: the tenant id makes each instance distinct (`service.instance.id`).
fn resource(otel: &OpenTelemetryOptions, tenant_name: &str, tenant_id: &str) -> Resource {
    let mut builder = Resource::builder()
        .with_service_name(otel.service_name.clone())
        .with_attribute(KeyValue::new(
            "tenant.name",
            if tenant_name.trim().is_empty() { "Default" } else { tenant_name }.to_string(),
        ))
        .with_attribute(KeyValue::new("tenant.id", tenant_id.to_string()));
    if !tenant_id.trim().is_empty() {
        builder = builder.with_attribute(KeyValue::new("service.instance.id", tenant_id.to_string()));
    }
    builder.build()
}

/// With HTTP/protobuf the signal path is appended to the base URL; an empty endpoint leaves the
/// standard `OTEL_EXPORTER_OTLP_*` variables (and their defaults) in charge.
fn http_endpoint(otel: &OpenTelemetryOptions, signal: &str) -> Option<String> {
    (!otel.endpoint.trim().is_empty()).then(|| format!("{}/v1/{signal}", otel.endpoint.trim().trim_end_matches('/')))
}

fn grpc_endpoint(otel: &OpenTelemetryOptions) -> Option<String> {
    (!otel.endpoint.trim().is_empty()).then(|| otel.endpoint.trim().to_string())
}

fn span_exporter(otel: &OpenTelemetryOptions) -> Result<SpanExporter, opentelemetry_otlp::ExporterBuildError> {
    match otel.protocol {
        OtlpTransport::Grpc => {
            let builder = SpanExporter::builder().with_tonic();
            match grpc_endpoint(otel) {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
        OtlpTransport::HttpProtobuf => {
            let builder = SpanExporter::builder().with_http().with_protocol(opentelemetry_otlp::Protocol::HttpBinary);
            match http_endpoint(otel, "traces") {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
    }
}

fn metric_exporter(otel: &OpenTelemetryOptions) -> Result<MetricExporter, opentelemetry_otlp::ExporterBuildError> {
    match otel.protocol {
        OtlpTransport::Grpc => {
            let builder = MetricExporter::builder().with_tonic();
            match grpc_endpoint(otel) {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
        OtlpTransport::HttpProtobuf => {
            let builder = MetricExporter::builder().with_http().with_protocol(opentelemetry_otlp::Protocol::HttpBinary);
            match http_endpoint(otel, "metrics") {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
    }
}

fn log_exporter(otel: &OpenTelemetryOptions) -> Result<LogExporter, opentelemetry_otlp::ExporterBuildError> {
    match otel.protocol {
        OtlpTransport::Grpc => {
            let builder = LogExporter::builder().with_tonic();
            match grpc_endpoint(otel) {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
        OtlpTransport::HttpProtobuf => {
            let builder = LogExporter::builder().with_http().with_protocol(opentelemetry_otlp::Protocol::HttpBinary);
            match http_endpoint(otel, "logs") {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
    }
}

/// Observable gauges over the metrics snapshot: same names and semantics as `/metrics`.
fn register_gauges(provider: &SdkMeterProvider, metrics: Arc<ScanMetrics>) {
    let meter = provider.meter(METER_NAME);

    let tenant_tags = |m: &ScanMetrics| {
        vec![
            KeyValue::new("tenant_name", m.tenant_name().to_string()),
            KeyValue::new("tenant_id", m.tenant_id().to_string()),
        ]
    };

    let m = metrics.clone();
    meter
        .i64_observable_gauge(metrics::DAYS_LEFT.0)
        .with_description(metrics::DAYS_LEFT.1)
        .with_callback(move |observer| {
            for c in &m.snapshot().credentials {
                let mut tags = vec![
                    KeyValue::new("app_name", c.app_name.clone()),
                    KeyValue::new("app_id", c.app_id.clone()),
                    KeyValue::new("key_id", c.key_id.clone()),
                    KeyValue::new("type", c.credential_type.as_str()),
                    KeyValue::new("credential_name", c.credential_name.clone()),
                ];
                tags.extend(tenant_tags(&m));
                observer.observe(c.days_left, &tags);
            }
        })
        .build();

    // The figures only exist once a scan has succeeded: an absent series is clearer than a misleading 0
    for (metric, read) in [
        (metrics::EXPIRED, (|s: &metrics::Snapshot| s.expired as i64) as fn(&metrics::Snapshot) -> i64),
        (metrics::EXPIRING_SOON, |s| s.expiring_soon as i64),
        (metrics::APPS_SCANNED, |s| s.apps_scanned as i64),
    ] {
        let m = metrics.clone();
        meter
            .i64_observable_gauge(metric.0)
            .with_description(metric.1)
            .with_callback(move |observer| {
                let snapshot = m.snapshot();
                if snapshot.has_result {
                    observer.observe(read(&snapshot), &tenant_tags(&m));
                }
            })
            .build();
    }

    let m = metrics.clone();
    meter
        .f64_observable_gauge(metrics::LAST_SCAN_TIMESTAMP.0)
        .with_description(metrics::LAST_SCAN_TIMESTAMP.1)
        .with_callback(move |observer| {
            let snapshot = m.snapshot();
            if snapshot.last_scan_success.is_some() {
                observer.observe(snapshot.last_scan_timestamp as f64, &tenant_tags(&m));
            }
        })
        .build();

    let m = metrics;
    meter
        .i64_observable_gauge(metrics::LAST_SCAN_SUCCESS.0)
        .with_description(metrics::LAST_SCAN_SUCCESS.1)
        .with_callback(move |observer| {
            if let Some(success) = m.snapshot().last_scan_success {
                observer.observe(i64::from(success), &tenant_tags(&m));
            }
        })
        .build();
}

#[cfg(test)]
mod tests;
