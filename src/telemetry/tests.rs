use super::*;

fn options(endpoint: &str) -> OpenTelemetryOptions {
    OpenTelemetryOptions { enabled: true, endpoint: endpoint.into(), ..Default::default() }
}

#[test]
fn http_endpoint_gets_the_signal_path_appended() {
    assert_eq!(http_endpoint(&options("http://alloy:4318"), "traces").as_deref(), Some("http://alloy:4318/v1/traces"));
    assert_eq!(http_endpoint(&options("http://alloy:4318/"), "logs").as_deref(), Some("http://alloy:4318/v1/logs"));
}

#[test]
fn empty_endpoint_defers_to_the_standard_variables() {
    assert_eq!(http_endpoint(&options(" "), "metrics"), None);
    assert_eq!(grpc_endpoint(&options("")), None);
    assert_eq!(grpc_endpoint(&options("http://alloy:4317")).as_deref(), Some("http://alloy:4317"));
}

#[test]
fn resource_identifies_the_tenant() {
    use opentelemetry::Key;
    let resource = resource(&OpenTelemetryOptions::default(), "Contoso", "tenant-id");
    let get = |k: &'static str| resource.get(&Key::from_static_str(k)).map(|v| v.to_string());

    assert_eq!(get("service.name").as_deref(), Some("entra-secret-watcher"));
    assert_eq!(get("service.instance.id").as_deref(), Some("tenant-id"));
    assert_eq!(get("tenant.name").as_deref(), Some("Contoso"));
    assert_eq!(get("tenant.id").as_deref(), Some("tenant-id"));
}
