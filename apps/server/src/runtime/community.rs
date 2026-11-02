use super::polling::{self, Event, Reporting, Worker};
use hito_core::Community;
use tokio::sync::watch;
use uuid::Uuid;

pub(super) const REPORTS: Worker = Worker {
    started: Event {
        name: "community_worker_started",
        message: "Community worker started",
    },
    polled: Event {
        name: "community_worker_polled",
        message: "Community worker checked pending reports",
    },
    failed: Event {
        name: "community_worker_failed",
        message: "Community worker failed",
    },
    stopped: Event {
        name: "community_worker_stopped",
        message: "Community worker stopped",
    },
    reporting: Reporting::Claimed,
};

#[tracing::instrument(level = "info", skip_all, fields(worker_id = %Uuid::new_v4()))]
pub(super) async fn worker(community: Community, stopped: watch::Receiver<bool>) {
    polling::run(REPORTS, stopped, |stop| {
        community.run_next_report_until(stop)
    })
    .await;
}
