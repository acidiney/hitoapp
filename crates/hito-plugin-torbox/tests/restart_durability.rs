#![cfg(feature = "test-support")]

#[path = "../../hito-debrid-torbox/tests/audio/fixtures.rs"]
pub mod fixtures;
#[path = "../../hito-plugin-all-debrid/tests/fixtures/http.rs"]
pub mod http;
#[path = "../../hito-debrid-torbox/tests/audio/owned_tls.rs"]
pub mod owned_tls;
#[path = "../../hito-debrid-torbox/tests/support/mod.rs"]
pub mod provider;
#[path = "../../hito-debrid-torbox/tests/audio/scripted_media.rs"]
pub mod scripted_media;
#[path = "support/state_inputs.rs"]
mod state_inputs;
#[path = "support/restart_workflow.rs"]
pub mod workflow;

use reqwest::StatusCode;
use std::time::Duration;
pub use workflow::COOKIE;
use workflow::{Fixture, assert_ready_status, assert_requests, complete};

#[tokio::test]
async fn ready_receipt_survives_owner_replacement_and_two_freshly_verified_gets() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = Fixture::start().await;
        let first = fixture.start_service().await;
        let saved = first.prepare().await;
        first.close().await;

        let second = fixture.start_service().await;
        let (status, bytes) = second.status(&saved).await;
        if status != StatusCode::OK {
            second.close().await;
            let _ = fixture.close().await;
            assert_eq!(
                status,
                StatusCode::OK,
                "the original preparation handle must survive service-owner replacement"
            );
            return;
        }
        assert_ready_status(&bytes, &saved);
        second.assert_replay_ready(&saved).await;
        let first_get_started = tokio::time::Instant::now();
        complete(second.audio().await).await;
        let first_get_finished = tokio::time::Instant::now();
        let second_get_started = tokio::time::Instant::now();
        complete(second.audio().await).await;
        let second_get_finished = tokio::time::Instant::now();
        second.assert_replay_ready(&saved).await;
        let (status, bytes) = second.status(&saved).await;
        assert_eq!(status, StatusCode::OK);
        assert_ready_status(&bytes, &saved);

        second.close().await;
        let (requests, media_requests) = fixture.close().await;
        assert_requests(
            &requests,
            [
                (first_get_started, first_get_finished),
                (second_get_started, second_get_finished),
            ],
        );
        assert_eq!(media_requests, 2);
    })
    .await
    .expect("the owned TorBox restart workflow exceeded its deadline");
}
