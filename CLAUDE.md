# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

**entra-secret-watcher (Rust)** is an Apache-2.0 tool that watches the client secrets and certificates of Microsoft
Entra ID app registrations and notifies before they expire. It is a Rust rewrite of the .NET project
[entra-secret-watcher](https://github.com/jerome-duchene/entra-secret-watcher): configuration, endpoints, metric names
and notification payloads must stay identical so both images are interchangeable. One container per tenant,
configured only through environment variables (`Section__Key`, see `src/config.rs`).

## Commands

```bash
cargo test                                   # all tests, no network or credentials needed
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings    # CI treats warnings as errors
docker build -t entra-secret-watcher .       # ~18 MB Alpine image, musl static binary
```

On this Windows machine the MSVC build tools are incomplete (no `lib\x64`, so `link.exe` cannot find `msvcrt.lib`):
build and test in a container instead, e.g.
`docker run --rm -v "${PWD}:/work" -v esw-cargo:/usr/local/cargo/registry -v esw-target:/target -e CARGO_TARGET_DIR=/target -w /work rust:latest cargo test`.

## Architecture (`src/`)

- `config.rs`: env → typed options, validation with all errors reported at once (messages mirror the .NET ones).
- `graph.rs`: minimal Graph client (client-credentials token cached, `applications` pagination, `sendMail`). No retry
  on purpose: `sendMail` is not idempotent. Base URLs are injectable for tests.
- `scanner.rs`: turns Graph applications into `ExpiringCredential`s within `ThresholdDays`.
- `job.rs`: one scan = scan → metrics → (dry-run log | dispatch).
- `notifications/`: `NotificationChannel` trait; `gotify`, `email`, `teams`, `webhook`; `NotificationDispatcher` isolates channel failures and handles grouped / per-credential reports.
- `scheduler.rs`: cron (5 fields, UTC, `croner`) + `trigger()`; scans never overlap (mutex).
- `metrics.rs`: single `Snapshot` rendered as Prometheus text (`/metrics`) and observed by the OTel gauges.
- `telemetry.rs`: tracing subscriber; OTLP traces/metrics/logs when `OpenTelemetry__Enabled=true`.
- `server.rs`: axum router (`/health`, `/metrics`, `POST /scan` only mapped when `Watcher__ManualTriggerSecret` is set, constant-time token check).
- `main.rs`: wiring and graceful shutdown.

## Conventions

- Unit tests sit in a sibling file declared with `#[cfg(test)] mod tests;` (e.g. `src/graph.rs` → `src/graph/tests.rs`; `use super::*;` keeps access to private items), so business code files stay free of tests (except the tiny `models.rs`). End-to-end scenarios that only use the public API live in `tests/`. HTTP is mocked with `wiremock`, routers are driven with `tower::ServiceExt::oneshot`, and `models::test_support` has shared fixtures.
- `rustfmt.toml` sets `max_width = 120`; run `cargo fmt --all` before committing.
- Keep `README.md` configuration tables and `CHANGELOG.md` (`[Unreleased]`) in sync with behaviour changes: the release workflow takes the GitHub Release notes from the matching CHANGELOG section.
- `grafana/` is shared with the .NET project and relies on the `esw_*` metric names and their labels: never rename them.
