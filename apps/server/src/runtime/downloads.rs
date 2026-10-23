use hito_core::Downloads;
use std::time::{Duration, Instant};
use tokio::sync::watch;

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot))]
pub(super) async fn worker(
    downloads: Downloads,
    mut stopped: watch::Receiver<bool>,
    worker_slot: u8,
) {
    tracing::info!(event = "download_worker_started", "Download worker started");
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *stopped.borrow() {
            break;
        }
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = interval.tick() => {}
        }
        let started = Instant::now();
        let result = downloads
            .run_next_job_until(async {
                let _ = stopped.changed().await;
            })
            .await;
        match result {
            Ok(claimed) => tracing::debug!(
                event = "download_worker_polled",
                claimed,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Download worker checked pending work"
            ),
            Err(error) => tracing::error!(
                event = "download_worker_failed",
                error_kind = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Download worker failed"
            ),
        }
    }
    tracing::info!(event = "download_worker_stopped", "Download worker stopped");
}

#[tracing::instrument(level = "info", skip_all)]
pub(super) async fn reconcile(downloads: Downloads, mut stopped: watch::Receiver<bool>) {
    tracing::info!(
        event = "download_reconciliation_started",
        "Playlist download reconciliation started"
    );
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *stopped.borrow() {
            break;
        }
        tokio::select! { biased; _ = stopped.changed() => break, _ = interval.tick() => {} }
        let started = Instant::now();
        let result = downloads
            .run_next_playlist_reconciliation_until(async {
                let _ = stopped.changed().await;
            })
            .await;
        match result {
            Ok(progressed) => tracing::debug!(
                event = "download_reconciliation_polled",
                progressed,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist download reconciliation checked work"
            ),
            Err(error) => tracing::error!(
                event = "download_reconciliation_failed",
                error_kind = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Playlist download reconciliation failed"
            ),
        }
    }
    tracing::info!(
        event = "download_reconciliation_stopped",
        "Playlist download reconciliation stopped"
    );
}
