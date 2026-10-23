#![cfg(feature = "test-support")]

pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_debrid_torbox::test_support::owned_tls;
pub use hito_debrid_torbox::test_support::provider;
pub use hito_debrid_torbox::test_support::scripted_media;
pub use hito_testkit::plugin_server as http;
#[path = "support/restart_uncertain_workflow.rs"]
mod workflow;

use reqwest::StatusCode;
use std::time::Duration;
use workflow::{Fixture, LostAcknowledgement};

pub const COOKIE: &str = "owned-private-torbox-uncertain-cookie";

#[tokio::test]
async fn received_mint_with_lost_acknowledgement_stays_unknown_after_owner_replacement() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (fixture, loss) = Fixture::start().await;
        let LostAcknowledgement { received, peer_closed, hold_acknowledgement: _held } = loss;
        let first = fixture.open().await;
        let operation = workflow::request();
        let (in_flight, completed) = {
            let mut pending = Box::pin(first.submit(&operation));
            tokio::select! {
                _ = &mut pending => panic!("mint must remain held before acknowledgement"),
                received = tokio::time::timeout(Duration::from_secs(2), received) => {
                    received.expect("mint must reach the owned provider").expect("mint witness must remain owned");
                }
            }
            let in_flight = first.submit(&operation).await;
            let completed = tokio::time::timeout(Duration::from_secs(12), pending)
                .await.expect("lost acknowledgement must return through the public preparation route");
            (in_flight, completed)
        };
        tokio::time::timeout(Duration::from_secs(2), peer_closed)
            .await.expect("the timed-out mint must close its provider peer")
            .expect("the provider peer-close witness must remain owned");

        let saved = in_flight.accepted_receipt(&operation);
        let mut polls = Vec::new();
        if let Some(receipt) = &saved { polls.push(first.status(receipt).await); }
        let before_replay = first.submit(&operation).await;
        let before_media = fixture.media_requests();
        first.close().await;

        let second = fixture.open().await;
        let mut replays = Vec::new();
        for _ in 0..2 {
            if let Some(receipt) = &saved { polls.push(second.status(receipt).await); }
            replays.push(second.submit(&operation).await);
        }
        let competing_operation = workflow::request();
        let competing = second.submit(&competing_operation).await;
        let audio = second.audio().await;
        if let Some(receipt) = &saved { polls.push(second.status(receipt).await); }
        replays.push(second.submit(&operation).await);
        let after_media = fixture.media_requests();
        second.close().await;
        let (requests, media_requests) = fixture.close().await;

        workflow::assert_requests(&requests);
        assert_eq!(before_media, 0);
        assert_eq!(after_media, 0);
        assert_eq!(media_requests, 0);
        let receipt = saved.expect("the in-flight operation must return its actual stable handle");
        in_flight.assert_unknown_submission(&operation, &receipt);
        completed.assert_unknown_submission(&operation, &receipt);
        before_replay.assert_unknown_submission(&operation, &receipt);
        assert_eq!(polls.len(), 4);
        for response in polls { response.assert_unknown_status(&receipt); }
        assert_eq!(replays.len(), 3);
        for response in replays { response.assert_unknown_submission(&operation, &receipt); }
        assert!(competing_operation.operation_id != operation.operation_id);
        competing.assert_closed_error(StatusCode::CONFLICT, "The operation belongs to another source");
        audio.assert_closed_error(StatusCode::SERVICE_UNAVAILABLE, "The source is unavailable");
    })
    .await.expect("the owned lost-acknowledgement restart workflow exceeded its deadline");
}
