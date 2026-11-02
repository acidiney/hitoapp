use super::polling::{self, Event, Reporting, Worker};
use hito_core::Catalog;
use tokio::sync::watch;

pub(super) const ARTWORK: Worker = Worker {
    started: Event {
        name: "artwork_worker_started",
        message: "Artwork worker started",
    },
    polled: Event {
        name: "artwork_worker_polled",
        message: "Artwork worker checked pending work",
    },
    failed: Event {
        name: "artwork_worker_failed",
        message: "Artwork worker failed",
    },
    stopped: Event {
        name: "artwork_worker_stopped",
        message: "Artwork worker stopped",
    },
    reporting: Reporting::Claimed,
};

pub(super) const COVER_ART: Worker = Worker {
    started: Event {
        name: "cover_art_worker_started",
        message: "Cover art worker started",
    },
    polled: Event {
        name: "cover_art_worker_polled",
        message: "Cover art worker checked pending work",
    },
    failed: Event {
        name: "cover_art_worker_failed",
        message: "Cover art worker failed",
    },
    stopped: Event {
        name: "cover_art_worker_stopped",
        message: "Cover art worker stopped",
    },
    reporting: Reporting::Claimed,
};

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
