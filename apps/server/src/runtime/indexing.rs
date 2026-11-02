use super::polling::{self, Event, Reporting, Worker};
use hito_core::{LibraryStorage, WatchStatus};
use tokio::sync::watch;

const INDEXING: Worker = Worker {
    started: Event {
        name: "worker_started",
        message: "Indexing worker started",
    },
    polled: Event {
        name: "worker_polled",
        message: "Indexing worker checked pending work",
    },
    failed: Event {
        name: "worker_failed",
        message: "Background worker failed",
    },
    stopped: Event {
        name: "worker_stopped",
        message: "Indexing worker stopped",
    },
    reporting: Reporting::Claimed,
};

pub(super) async fn worker(storage: LibraryStorage, stopped: watch::Receiver<bool>) {
    polling::run(INDEXING, stopped, |stop| storage.run_next_job_until(stop)).await;
}

pub(super) async fn folder_watching(storage: LibraryStorage, stopped: watch::Receiver<bool>) {
    let (status, _) = watch::channel(WatchStatus::new());
    storage.watch_folders(stopped, status).await;
}
