# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Rust rewrite of [entra-secret-watcher](https://github.com/jerome-duchene/entra-secret-watcher) (.NET),
  feature-compatible with its 0.5.0 release: scheduled and on-demand (`POST /scan`) scans, Gotify, Email (Graph),
  Teams and generic webhook channels, dry-run, Prometheus `/metrics`, OTLP push of traces, metrics and logs.
- `Server__Listen` to choose the listen address.
