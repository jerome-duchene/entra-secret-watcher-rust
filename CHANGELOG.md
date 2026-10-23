# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Application health metrics (`esw_scans_total`, `esw_notifications_total`, `esw_graph_requests_total`,
  `esw_last_scan_duration_seconds`, `esw_process_start_time_seconds`, `esw_build_info`) and a second Grafana
  dashboard dedicated to the health of the application (restarts, scans, Graph, notification delivery, errors).
- A rejected webhook call now reports the response body of the receiver in the error.
- Rust rewrite of [entra-secret-watcher](https://github.com/jerome-duchene/entra-secret-watcher) (.NET),
  feature-compatible with its 0.5.0 release: scheduled and on-demand (`POST /scan`) scans, Gotify, Email (Graph),
  Teams and generic webhook channels, dry-run, Prometheus `/metrics`, OTLP push of traces, metrics and logs.
- `Server__Listen` to choose the listen address.
