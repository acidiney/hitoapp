use super::polling::{self, Reporting, Worker, worker};
use hito_core::Catalog;
use tokio::sync::watch;

pub(super) const PREPARATION: Worker = worker!(
    "source_preparation_worker",
    "Source preparation worker",
    Reporting::Claimed
);

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot))]
pub(super) async fn worker(catalog: Catalog, stopped: watch::Receiver<bool>, worker_slot: u8) {
    polling::run(PREPARATION, stopped, |stop| {
        catalog.run_next_source_preparation_until(stop)
    })
    .await;
}
