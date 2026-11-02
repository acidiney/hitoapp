use super::polling::{self, Reporting, Worker, worker};
use hito_core::{LibraryStorage, WatchStatus};
use tokio::sync::watch;

pub(super) const INDEXING: Worker = worker!("worker", "Indexing worker", Reporting::Claimed)
    .with_failed_message("Background worker failed");

pub(super) async fn worker(storage: LibraryStorage, stopped: watch::Receiver<bool>) {
    polling::run(INDEXING, stopped, |stop| storage.run_next_job_until(stop)).await;
}

pub(super) async fn folder_watching(storage: LibraryStorage, stopped: watch::Receiver<bool>) {
    let (status, _) = watch::channel(WatchStatus::new());
    storage.watch_folders(stopped, status).await;
}
