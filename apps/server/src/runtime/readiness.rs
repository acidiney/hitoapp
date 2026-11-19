use super::polling::{self, Reporting, Worker, worker};
use hito_core::Catalog;
use tokio::sync::watch;

pub(super) const READINESS: Worker = worker!(
    "source_readiness_worker",
    "Background source preparation worker",
    Reporting::Claimed
);

pub(super) async fn worker(catalog: Catalog, stopped: watch::Receiver<bool>) {
    if let Err(error) = catalog.queue_source_readiness().await {
        tracing::error!(
            event = "source_readiness_queue_failed",
            error_kind = error.code(),
            "Background source preparation walks could not be queued"
        );
    }
    polling::run(READINESS, stopped, |stop| {
        catalog.run_next_source_readiness_until(stop)
    })
    .await;
}
