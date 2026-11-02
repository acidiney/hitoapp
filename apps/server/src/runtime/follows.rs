use super::polling::{self, Event, Reporting, Worker};
use hito_core::Collection;
use tokio::sync::watch;

pub(super) const FOLLOW: Worker = Worker {
    started: Event {
        name: "follow_worker_started",
        message: "Playlist follow worker started",
    },
    polled: Event {
        name: "follow_worker_polled",
        message: "Playlist follow worker checked pending work",
    },
    failed: Event {
        name: "follow_worker_failed",
        message: "Playlist follow worker failed",
    },
    stopped: Event {
        name: "follow_worker_stopped",
        message: "Playlist follow worker stopped",
    },
    reporting: Reporting::Claimed,
};

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot))]
pub(super) async fn worker(
    collection: Collection,
    stopped: watch::Receiver<bool>,
    worker_slot: u8,
) {
    polling::run(FOLLOW, stopped, |stop| {
        collection.run_next_follow_refresh_until(stop)
    })
    .await;
}
