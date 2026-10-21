#![cfg(feature = "test-support")]

#[path = "../../hito-debrid-torbox/tests/audio/fixtures.rs"]
pub mod fixtures;
#[path = "../../hito-plugin-all-debrid/tests/fixtures/http.rs"]
pub mod http;
#[path = "../../hito-debrid-torbox/tests/audio/owned_tls.rs"]
pub mod owned_tls;
#[path = "../../hito-debrid-torbox/tests/support/mod.rs"]
pub mod provider;
#[path = "support/restart_workflow.rs"]
pub mod ready_workflow;
#[path = "../../hito-debrid-torbox/tests/audio/scripted_media.rs"]
pub mod scripted_media;
#[path = "support/state_inputs.rs"]
mod state_inputs;
#[path = "support/restart_catalog_workflow.rs"]
pub mod workflow;

#[path = "support/restart_catalog_entries.rs"]
pub mod entries;
#[path = "support/restart_catalog_protocol.rs"]
mod protocol;
#[path = "support/restart_catalog_uncertain.rs"]
mod uncertain;

use std::time::Duration;

pub const COOKIE: &str = "owned-private-torbox-catalog-cookie";

#[tokio::test]
async fn ready_receipt_survives_an_unrelated_catalog_addition() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = workflow::Fixture::start().await;
        let first = fixture
            .open(false)
            .await
            .expect("the original private catalog owner must open");
        let saved = first.prepare().await;
        first.close().await;

        let second = match fixture.open(true).await {
            Ok(second) => second,
            Err(error) => {
                let (requests, media_requests) = fixture.close().await;
                workflow::assert_requests(&requests, None);
                assert_eq!(media_requests, 0);
                panic!(
                    "an unrelated addition must preserve the unchanged retained source: {error:?}"
                );
            }
        };
        let added_detail = second.added_detail().await;
        let status = second.status(&saved).await;
        let replay = second.replay(&saved).await;
        let started = tokio::time::Instant::now();
        ready_workflow::complete(second.audio().await).await;
        let finished = tokio::time::Instant::now();
        second.close().await;
        let (requests, media_requests) = fixture.close().await;

        workflow::assert_added_detail(added_detail);
        workflow::assert_ready_status(status, &saved);
        workflow::assert_ready_replay(replay, &saved);
        workflow::assert_requests(&requests, Some((started, finished)));
        assert_eq!(media_requests, 1);
    })
    .await
    .expect("the owned catalog-addition restart workflow exceeded its deadline");
}

#[tokio::test]
async fn ready_receipt_survives_catalog_reorder_and_unrelated_removal() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = workflow::Fixture::start_removal().await;
        let first = fixture
            .open_catalog(entries::Evolution::Surrounded)
            .await
            .expect("the original surrounded catalog owner must open");
        let saved = first.prepare().await;
        first.close().await;
        let mut statuses = Vec::new();
        let mut replays = Vec::new();
        let mut details = Vec::new();
        let mut windows = Vec::new();
        for selection in [
            entries::Evolution::Reordered,
            entries::Evolution::RemovedUnrelated,
        ] {
            let owner = fixture
                .open_catalog(selection)
                .await
                .expect("an unchanged source must survive catalog evolution");
            details.push(owner.added_detail().await);
            statuses.push(owner.status(&saved).await);
            replays.push(owner.replay(&saved).await);
            let started = tokio::time::Instant::now();
            ready_workflow::complete(owner.audio().await).await;
            windows.push((started, tokio::time::Instant::now()));
            owner.close().await;
        }
        let (requests, media_requests) = fixture.close().await;
        assert_eq!(statuses.len(), 2);
        for response in statuses {
            workflow::assert_ready_status(response, &saved);
        }
        assert_eq!(replays.len(), 2);
        for response in replays {
            workflow::assert_ready_replay(response, &saved);
        }
        assert_eq!(details.len(), 2);
        let mut details = details.into_iter();
        workflow::assert_added_detail(details.next().unwrap());
        protocol::assert_closed_error(
            details.next().unwrap(),
            reqwest::StatusCode::NOT_FOUND,
            "The requested source is unavailable",
        );
        assert_eq!(windows.len(), 2);
        ready_workflow::assert_requests(&requests, [windows[0], windows[1]]);
        assert_eq!(media_requests, 2);
    })
    .await
    .expect("the owned catalog reorder/removal workflow exceeded its deadline");
}

#[tokio::test]
async fn unknown_claim_survives_catalog_removal_reuse_and_reintroduction() {
    tokio::time::timeout(Duration::from_secs(30), uncertain::run())
        .await
        .expect("the owned catalog Unknown compatibility workflow exceeded its deadline");
}
