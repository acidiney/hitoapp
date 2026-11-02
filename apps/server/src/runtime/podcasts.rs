use super::polling::{self, Event, Reporting, Worker};
use hito_core::Podcasts;
use tokio::sync::watch;

pub(super) const REFRESH: Worker = Worker {
    started: Event {
        name: "podcast_worker_started",
        message: "Podcast refresh worker started",
    },
    polled: Event {
        name: "podcast_worker_polled",
        message: "Podcast refresh worker checked pending work",
    },
    failed: Event {
        name: "podcast_worker_failed",
        message: "Podcast refresh worker failed",
    },
    stopped: Event {
        name: "podcast_worker_stopped",
        message: "Podcast refresh worker stopped",
    },
    reporting: Reporting::ClaimedWithOutcome,
};

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot, worker_id = %uuid::Uuid::new_v4()))]
pub(super) async fn worker(podcasts: Podcasts, stopped: watch::Receiver<bool>, worker_slot: u8) {
    polling::run(REFRESH, stopped, |stop| {
        podcasts.run_next_refresh_until(stop)
    })
    .await;
}
