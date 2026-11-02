use super::polling::{self, Reporting, Worker, worker};
use hito_core::Downloads;
use tokio::sync::watch;

pub(super) const TRANSFER: Worker =
    worker!("download_worker", "Download worker", Reporting::Claimed);

pub(super) const RECONCILIATION: Worker = worker!(
    "download_reconciliation",
    "Playlist download reconciliation",
    Reporting::Progressed
)
.with_polled_message("Playlist download reconciliation checked work");

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot))]
pub(super) async fn worker(downloads: Downloads, stopped: watch::Receiver<bool>, worker_slot: u8) {
    polling::run(TRANSFER, stopped, |stop| downloads.run_next_job_until(stop)).await;
}

#[tracing::instrument(level = "info", skip_all)]
pub(super) async fn reconcile(downloads: Downloads, stopped: watch::Receiver<bool>) {
    polling::run(RECONCILIATION, stopped, |stop| {
        downloads.run_next_playlist_reconciliation_until(stop)
    })
    .await;
}
