use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use entra_secret_watcher::config::Config;
use entra_secret_watcher::graph::GraphClient;
use entra_secret_watcher::job::CredentialScanJob;
use entra_secret_watcher::metrics::ScanMetrics;
use entra_secret_watcher::notifications::email::EmailChannel;
use entra_secret_watcher::notifications::gotify::GotifyChannel;
use entra_secret_watcher::notifications::teams::TeamsChannel;
use entra_secret_watcher::notifications::webhook::WebhookChannel;
use entra_secret_watcher::notifications::{NotificationChannel, NotificationDispatcher};
use entra_secret_watcher::scanner::GraphCredentialScanner;
use entra_secret_watcher::scheduler::ScanScheduler;
use entra_secret_watcher::{server, telemetry};
use tracing::info;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e:#}");
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config) -> Result<()> {
    let metrics = Arc::new(ScanMetrics::new(&config.entra.tenant_name, &config.entra.tenant_id));
    let _telemetry = telemetry::init(&config, metrics.clone())?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(100))
        .user_agent(concat!("entra-secret-watcher/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let graph = Arc::new(GraphClient::new(http.clone(), config.entra.clone()));

    let notification = config.notification.clone();
    let channels: Vec<Arc<dyn NotificationChannel>> = vec![
        Arc::new(GotifyChannel::new(notification.gotify, http.clone())),
        Arc::new(EmailChannel::new(notification.email, graph.clone())),
        Arc::new(TeamsChannel::new(notification.teams, http.clone())),
        Arc::new(WebhookChannel::new(notification.webhook, http)),
    ];

    let job = Arc::new(CredentialScanJob::new(
        GraphCredentialScanner::new(graph, config.entra.clone(), config.watcher.clone()),
        NotificationDispatcher::new(channels, config.watcher.grouped_report),
        metrics.clone(),
        config.watcher.clone(),
    ));
    let scheduler = ScanScheduler::new(&config.watcher.cron_schedule, job, config.watcher.run_on_startup)?;

    let app =
        server::router(config.entra.tenant_name.clone(), metrics, config.watcher.manual_trigger_secret.clone(), {
            let scheduler = scheduler.clone();
            Arc::new(move || scheduler.trigger())
        });

    let listener = tokio::net::TcpListener::bind(config.server.listen)
        .await
        .with_context(|| format!("Cannot listen on {}", config.server.listen))?;
    info!(
        "entra-secret-watcher {} listening on {} (tenant {}, schedule '{}' UTC)",
        env!("CARGO_PKG_VERSION"),
        config.server.listen,
        config.entra.tenant_name,
        config.watcher.cron_schedule
    );

    tokio::spawn(scheduler.run());

    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    info!("Shutting down");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
