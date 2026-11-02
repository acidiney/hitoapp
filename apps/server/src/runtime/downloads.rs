use super::polling::{self, Event, Reporting, Worker};
use hito_core::Downloads;
use tokio::sync::watch;

pub(super) const TRANSFER: Worker = Worker {
    started: Event {
        name: "download_worker_started",
        message: "Download worker started",
    },
    polled: Event {
        name: "download_worker_polled",
        message: "Download worker checked pending work",
    },
    failed: Event {
        name: "download_worker_failed",
        message: "Download worker failed",
    },
    stopped: Event {
        name: "download_worker_stopped",
        message: "Download worker stopped",
    },
    reporting: Reporting::Claimed,
};

pub(super) const RECONCILIATION: Worker = Worker {
    started: Event {
        name: "download_reconciliation_started",
        message: "Playlist download reconciliation started",
    },
    polled: Event {
        name: "download_reconciliation_polled",
        message: "Playlist download reconciliation checked work",
    },
    failed: Event {
        name: "download_reconciliation_failed",
        message: "Playlist download reconciliation failed",
    },
    stopped: Event {
        name: "download_reconciliation_stopped",
        message: "Playlist download reconciliation stopped",
    },
    reporting: Reporting::Progressed,
};

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
