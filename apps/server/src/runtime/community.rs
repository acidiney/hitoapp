use hito_core::Community;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use uuid::Uuid;

#[tracing::instrument(level = "info", skip_all, fields(worker_id = %Uuid::new_v4()))]
pub(super) async fn worker(community: Community, mut stopped: watch::Receiver<bool>) {
    tracing::info!(
        event = "community_worker_started",
        "Community worker started"
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
        let result = community
            .run_next_report_until(async {
                let _ = stopped.changed().await;
            })
            .await;
        match result {
            Ok(claimed) => tracing::debug!(
                event = "community_worker_polled",
                claimed,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Community worker checked pending reports"
            ),
            Err(error) => tracing::error!(
                event = "community_worker_failed",
                error_kind = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Community worker failed"
            ),
        }
    }
    tracing::info!(
        event = "community_worker_stopped",
        "Community worker stopped"
    );
}
