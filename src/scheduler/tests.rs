use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct FakeTask {
    running: AtomicUsize,
    max_concurrent: AtomicUsize,
    runs: AtomicUsize,
    fail: bool,
}

#[async_trait]
impl ScanTask for FakeTask {
    async fn run(&self) -> Result<()> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_concurrent.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.runs.fetch_add(1, Ordering::SeqCst);
        if self.fail { anyhow::bail!("boom") } else { Ok(()) }
    }
}

#[test]
fn invalid_cron_is_rejected() {
    assert!(ScanScheduler::new("nope", Arc::new(FakeTask::default()), false).is_err());
}

#[tokio::test]
async fn triggered_scans_never_overlap() {
    let task = Arc::new(FakeTask::default());
    let scheduler = ScanScheduler::new("0 8 * * *", task.clone(), false).unwrap();

    for _ in 0..3 {
        scheduler.trigger();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(task.runs.load(Ordering::SeqCst), 3);
    assert_eq!(task.max_concurrent.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_failing_scan_does_not_break_the_scheduler() {
    let task = Arc::new(FakeTask { fail: true, ..Default::default() });
    let scheduler = ScanScheduler::new("0 8 * * *", task.clone(), false).unwrap();

    scheduler.trigger();
    scheduler.trigger();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(task.runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn run_on_startup_scans_immediately() {
    let task = Arc::new(FakeTask::default());
    let scheduler = ScanScheduler::new("0 8 * * *", task.clone(), true).unwrap();

    let handle = tokio::spawn(scheduler.run());
    tokio::time::sleep(Duration::from_millis(150)).await;
    handle.abort();

    assert_eq!(task.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn without_run_on_startup_nothing_runs_before_the_schedule() {
    let task = Arc::new(FakeTask::default());
    // Far enough in the future that it cannot fire during the test
    let scheduler = ScanScheduler::new("0 0 1 1 *", task.clone(), false).unwrap();

    let handle = tokio::spawn(scheduler.run());
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.abort();

    assert_eq!(task.runs.load(Ordering::SeqCst), 0);
}
