use super::polling::{self, Reporting, Worker, worker};
use hito_core::Podcasts;
use tokio::sync::watch;

pub(super) const REFRESH: Worker = worker!(
    "podcast_worker",
    "Podcast refresh worker",
    Reporting::ClaimedWithOutcome
);

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot, worker_id = %uuid::Uuid::new_v4()))]
pub(super) async fn worker(podcasts: Podcasts, stopped: watch::Receiver<bool>, worker_slot: u8) {
    polling::run(REFRESH, stopped, |stop| {
        podcasts.run_next_refresh_until(stop)
    })
    .await;
}
