use super::polling::{self, Event, Reporting, Worker};
use hito_core::Catalog;
use tokio::sync::watch;

const PREPARATION: Worker = Worker {
    started: Event {
        name: "source_preparation_worker_started",
        message: "Source preparation worker started",
    },
    polled: Event {
        name: "source_preparation_worker_polled",
        message: "Source preparation worker checked pending work",
    },
    failed: Event {
        name: "source_preparation_worker_failed",
        message: "Source preparation worker failed",
    },
    stopped: Event {
        name: "source_preparation_worker_stopped",
        message: "Source preparation worker stopped",
    },
    reporting: Reporting::Claimed,
};

#[tracing::instrument(level = "info", skip_all, fields(worker_slot = worker_slot))]
pub(super) async fn worker(catalog: Catalog, stopped: watch::Receiver<bool>, worker_slot: u8) {
    polling::run(PREPARATION, stopped, |stop| {
        catalog.run_next_source_preparation_until(stop)
    })
    .await;
}
