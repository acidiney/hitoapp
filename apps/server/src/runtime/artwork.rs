use super::polling::{self, Reporting, Worker, worker};
use hito_core::Catalog;
use tokio::sync::watch;

pub(super) const ARTWORK: Worker = worker!("artwork_worker", "Artwork worker", Reporting::Claimed);

pub(super) const COVER_ART: Worker =
    worker!("cover_art_worker", "Cover art worker", Reporting::Claimed);

pub(super) async fn worker(catalog: Catalog, stopped: watch::Receiver<bool>) {
    polling::run(ARTWORK, stopped, |stop| {
        catalog.run_next_artwork_job_until(stop)
    })
    .await;
}

pub(super) async fn cover_art_worker(catalog: Catalog, stopped: watch::Receiver<bool>) {
    polling::run(COVER_ART, stopped, |stop| {
        catalog.run_next_cover_art_lookup_until(stop)
    })
    .await;
}
