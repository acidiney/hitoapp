use super::polling::{self, Reporting, Worker, worker};
use hito_core::Community;
use tokio::sync::watch;
use uuid::Uuid;

pub(super) const REPORTS: Worker =
    worker!("community_worker", "Community worker", Reporting::Claimed)
        .with_polled_message("Community worker checked pending reports");

#[tracing::instrument(level = "info", skip_all, fields(worker_id = %Uuid::new_v4()))]
pub(super) async fn worker(community: Community, stopped: watch::Receiver<bool>) {
    polling::run(REPORTS, stopped, |stop| {
        community.run_next_report_until(stop)
    })
    .await;
}
