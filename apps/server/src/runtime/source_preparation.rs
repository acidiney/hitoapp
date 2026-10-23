use hito_core::Catalog;
use std::time::{Duration, Instant};
use tokio::sync::watch;

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot))]
pub(super) async fn worker(catalog: Catalog, mut stopped: watch::Receiver<bool>, worker_slot: u8) {
    tracing::info!(
        event = "source_preparation_worker_started",
        "Source preparation worker started"
    );
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *stopped.borrow() {
            break;
        }
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = interval.tick() => {},
        }
        let started = Instant::now();
        let result = catalog
            .run_next_source_preparation_until(async {
                let _ = stopped.changed().await;
            })
            .await;
        match result {
            Ok(claimed) => tracing::debug!(
                event = "source_preparation_worker_polled",
                claimed,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Source preparation worker checked pending work"
            ),
            Err(error) => tracing::error!(
                event = "source_preparation_worker_failed",
                error_kind = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Source preparation worker failed"
            ),
        }
    }
    tracing::info!(
        event = "source_preparation_worker_stopped",
        "Source preparation worker stopped"
    );
}
