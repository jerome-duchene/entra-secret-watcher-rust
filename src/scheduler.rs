use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use croner::Cron;
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Something the scheduler can run: in production the credential scan job.
#[async_trait]
pub trait ScanTask: Send + Sync + 'static {
    async fn run(&self) -> Result<()>;
}

#[async_trait]
impl ScanTask for crate::job::CredentialScanJob {
    async fn run(&self) -> Result<()> {
        self.execute().await
    }
}

/// Runs the scan on the `Watcher:CronSchedule` (UTC), and on demand via [`ScanScheduler::trigger`].
/// Scans never overlap: a run started while another one is in progress waits for it.
pub struct ScanScheduler {
    cron: Cron,
    task: Arc<dyn ScanTask>,
    run_on_startup: bool,
    lock: Mutex<()>,
}

impl ScanScheduler {
    pub fn new(cron_schedule: &str, task: Arc<dyn ScanTask>, run_on_startup: bool) -> Result<Arc<Self>> {
        Ok(Arc::new(Self { cron: cron_schedule.parse::<Cron>()?, task, run_on_startup, lock: Mutex::new(()) }))
    }

    /// Never returns under normal operation; drop the future to stop the schedule.
    pub async fn run(self: Arc<Self>) {
        if self.run_on_startup {
            self.run_once().await;
        }

        loop {
            let next = match self.cron.find_next_occurrence(&Utc::now(), false) {
                Ok(next) => next,
                Err(e) => {
                    warn!("The cron schedule has no further occurrence ({e}); the scheduler stops.");
                    return;
                }
            };

            // Wait in chunks of at most one hour so a suspended machine or a clock change is noticed
            while let Ok(wait) = (next - Utc::now()).to_std() {
                if wait.is_zero() {
                    break;
                }
                tokio::time::sleep(wait.min(Duration::from_secs(3600))).await;
            }

            self.run_once().await;
        }
    }

    /// Starts a scan in the background.
    pub fn trigger(self: &Arc<Self>) {
        let scheduler = Arc::clone(self);
        tokio::spawn(async move { scheduler.run_once().await });
    }

    async fn run_once(&self) {
        let _guard = self.lock.lock().await;
        if let Err(e) = self.task.run().await {
            // Already logged as an error by the job; the next run is the retry
            debug!("Scheduled scan failed: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests;
