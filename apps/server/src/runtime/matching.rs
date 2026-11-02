use super::polling::{self, Event, Reporting, Worker};
use hito_core::ArtistCatalog;
use tokio::sync::watch;

pub(super) const MATCHING: Worker = Worker {
    started: Event {
        name: "matching_worker_started",
        message: "Artist matching worker started",
    },
    polled: Event {
        name: "matching_worker_polled",
        message: "Artist matching worker checked pending work",
    },
    failed: Event {
        name: "matching_worker_failed",
        message: "Artist matching worker failed",
    },
    stopped: Event {
        name: "matching_worker_stopped",
        message: "Artist matching worker stopped",
    },
    reporting: Reporting::Claimed,
};

pub(super) async fn worker(artists: ArtistCatalog, stopped: watch::Receiver<bool>) {
    if let Err(error) = artists.queue_first_automatic_matching().await {
        tracing::error!(
            event = "first_automatic_matching_failed",
            error_kind = error.code(),
            "First automatic artist matching walks could not be queued"
        );
    }
    polling::run(MATCHING, stopped, |stop| {
        artists.run_next_automatic_matching_until(stop)
    })
    .await;
}
