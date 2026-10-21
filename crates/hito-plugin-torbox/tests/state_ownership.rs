#![cfg(feature = "test-support")]

#[path = "support/restart_catalog_entries.rs"]
pub mod entries;
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

use hito_plugin_torbox::{InitError, StateError};
use std::time::Duration;

pub const COOKIE: &str = "owned-private-torbox-exclusive-owner-cookie";

#[tokio::test]
async fn second_owner_is_rejected_without_harming_ready_receipt_or_joined_replacement() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = workflow::Fixture::start_removal().await;
        let first = fixture.open(false).await.expect("the original private state owner must open");
        let saved = first.prepare().await;
        let second_outcome = match fixture.open(false).await {
            Err(InitError::State(StateError::AlreadyOwned)) => "already_owned",
            Err(_) => "other_error",
            Ok(unexpected) => {
                unexpected.close().await;
                "opened"
            }
        };
        workflow::assert_ready_status(first.status(&saved).await, &saved);
        workflow::assert_ready_replay(first.replay(&saved).await, &saved);
        let first_get_started = tokio::time::Instant::now();
        ready_workflow::complete(first.audio().await).await;
        let first_get_finished = tokio::time::Instant::now();
        workflow::assert_ready_status(first.status(&saved).await, &saved);
        first.close().await;

        let replacement = fixture.open(false).await.expect("joined close must release the private state owner");
        workflow::assert_ready_status(replacement.status(&saved).await, &saved);
        workflow::assert_ready_replay(replacement.replay(&saved).await, &saved);
        let replacement_get_started = tokio::time::Instant::now();
        ready_workflow::complete(replacement.audio().await).await;
        let replacement_get_finished = tokio::time::Instant::now();
        workflow::assert_ready_status(replacement.status(&saved).await, &saved);
        workflow::assert_ready_replay(replacement.replay(&saved).await, &saved);
        replacement.close().await;
        let (requests, media_requests) = fixture.close().await;

        eprintln!("exclusive_owner_evidence second_owner_outcome={second_outcome} provider_requests={} mint_requests={} media_requests={media_requests}",
            requests.len(), requests.iter().filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?")).count());
        assert_eq!(second_outcome, "already_owned", "a second Service::open must reject the live private state owner");
        ready_workflow::assert_requests(&requests, [
            (first_get_started, first_get_finished),
            (replacement_get_started, replacement_get_finished),
        ]);
        assert_eq!(media_requests, 2);
    }).await.expect("the public exclusive-owner workflow exceeded its deadline");
}
