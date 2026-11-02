use super::polling::{self, Reporting, Worker, worker};
use hito_core::Collection;
use tokio::sync::watch;

pub(super) const FOLLOW: Worker = worker!(
    "follow_worker",
    "Playlist follow worker",
    Reporting::Claimed
);

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
