use hito_core::Podcasts;
use std::time::{Duration, Instant};
use tokio::sync::watch;

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot, worker_id = %uuid::Uuid::new_v4()))]
pub(super) async fn worker(
    podcasts: Podcasts,
    mut stopped: watch::Receiver<bool>,
    worker_slot: u8,
) {
    tracing::info!(
        event = "podcast_worker_started",
        "Podcast refresh worker started"
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
            _ = interval.tick() => {}
        }
        let started = Instant::now();
        let result = podcasts
            .run_next_refresh_until(async {
                let _ = stopped.changed().await;
            })
            .await;
        match result {
            Ok(claimed) => tracing::debug!(
                event = "podcast_worker_polled",
                claimed,
                outcome = if claimed { "processed" } else { "idle" },
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Podcast refresh worker checked pending work"
            ),
            Err(error) => tracing::error!(
                event = "podcast_worker_failed",
                error_kind = error.code(),
                outcome = "failed",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Podcast refresh worker failed"
            ),
        }
    }
    tracing::info!(
        event = "podcast_worker_stopped",
        outcome = "stopped",
        "Podcast refresh worker stopped"
    );
}
