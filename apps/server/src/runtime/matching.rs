use super::polling::{self, Reporting, Worker, worker};
use hito_core::ArtistCatalog;
use tokio::sync::watch;

pub(super) const MATCHING: Worker = worker!(
    "matching_worker",
    "Artist matching worker",
    Reporting::Claimed
);

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
