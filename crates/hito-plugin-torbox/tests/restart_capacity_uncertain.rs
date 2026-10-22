#![cfg(feature = "test-support")]

#[path = "support/restart_catalog_entries.rs"]
#[allow(dead_code)]
mod catalog_entries;
#[path = "support/restart_capacity_entries.rs"]
mod entries;
pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_debrid_torbox::test_support::owned_tls;
pub use hito_testkit::plugin_server as http;
#[path = "support/restart_capacity_provider.rs"]
#[allow(dead_code)]
mod provider;
pub use hito_debrid_torbox::test_support::provider as uncertain_provider;
pub use hito_debrid_torbox::test_support::scripted_media;
#[path = "support/restart_capacity_uncertain_workflow.rs"]
mod workflow;

use reqwest::StatusCode;
use std::time::Duration;
use workflow::{
    Fixture, LostAcknowledgement, assert_closed_capacity, assert_unknown_status,
    assert_unknown_submission,
};

pub const COOKIE: &str = "owned-private-torbox-capacity-uncertain-cookie";

#[tokio::test]
async fn sqlite_capacity_retains_unknown_without_remint_after_owner_replacement() {
    tokio::time::timeout(Duration::from_secs(300), async {
        let started = tokio::time::Instant::now();
        let progress = |stage: &str, batch: usize, accepted: usize, counts: provider::Counts| {
            eprintln!("unknown_capacity_progress stage={stage} batch={batch} accepted={accepted} later_provider_requests={} later_mint_requests={} elapsed_ms={}",
                counts.requests, counts.mints, started.elapsed().as_millis());
        };
        progress("fixture_starting", 0, 0, provider::Counts::default());
        let (mut fixture, loss) = Fixture::start().await;
        let LostAcknowledgement { received, peer_closed, hold_acknowledgement } = loss;
        let first = fixture.open_uncertain().await;
        let operation = workflow::unknown_request();
        let (in_flight, completed) = {
            let mut pending = Box::pin(first.submit(&operation));
            tokio::select! {
                _ = &mut pending => panic!("the original mint acknowledgement must remain held"),
                received = tokio::time::timeout(Duration::from_secs(2), received) => {
                    received.expect("the mint must reach the owned uncertain provider")
                        .expect("the original mint witness must remain owned");
                }
            }
            let in_flight = first.submit(&operation).await;
            let completed = tokio::time::timeout(Duration::from_secs(12), pending).await
                .expect("the lost acknowledgement must return through the public preparation route");
            (in_flight, completed)
        };
        tokio::time::timeout(Duration::from_secs(2), peer_closed).await
            .expect("the timed-out mint must close its provider peer")
            .expect("the original peer-close witness must remain owned");
        let saved = workflow::saved(&operation, &in_flight);
        assert_unknown_submission(in_flight, &saved);
        assert_unknown_submission(completed, &saved);
        assert_unknown_status(first.status(&saved).await, &saved);
        assert_unknown_submission(first.submit(&saved.request).await, &saved);
        workflow::assert_unavailable_audio(first.audio().await);
        assert_eq!(fixture.media_requests(), 0);
        first.close().await.expect("the original HTTP and SQLite owners must fully join");
        let (original_requests, original_mints) = fixture.start_capacity().await;
        drop(hold_acknowledgement);
        assert_eq!((original_requests, original_mints), (2, 1));
        progress("uncertain_owner_and_provider_joined", 0, 0, fixture.provider_counts());

        let mut batch = 0;
        let mut owner = fixture.open(batch).await;
        assert_unknown_status(owner.status(&saved).await, &saved);
        assert_unknown_submission(owner.submit(&saved.request).await, &saved);
        assert_eq!(fixture.provider_counts(), provider::Counts::default());
        owner.prepare_ready_control().await;
        assert_eq!(fixture.provider_counts().requests, 2);
        assert_eq!(fixture.provider_counts().mints, 1);
        progress("ready_control", batch, 0, fixture.provider_counts());

        let mut accepted = 0;
        let (request, rejected, before_rejection) = 'fill: loop {
            for index in workflow::batch(batch) {
                let request = entries::request(index);
                let before = fixture.provider_counts();
                let response = owner.submit(&request).await;
                if response.0 != StatusCode::OK { break 'fill (request, response, before); }
                workflow::assert_preparing(response, &request);
                accepted += 1;
                assert_eq!(fixture.provider_counts().requests, before.requests + 1);
                assert_eq!(fixture.provider_counts().mints, 1);
                if accepted % 64 == 0 { progress("filling", batch, accepted, fixture.provider_counts()); }
            }
            assert!(accepted + 2 < 1024, "physical SQLite capacity must precede the receipt-count guard");
            progress("owner_closing", batch, accepted, fixture.provider_counts());
            owner.close().await.expect("each filler batch must join its real HTTP and SQLite owners");
            progress("owner_closed", batch, accepted, fixture.provider_counts());
            batch += 1;
            progress("owner_opening", batch, accepted, fixture.provider_counts());
            owner = fixture.open(batch).await;
            assert_unknown_status(owner.status(&saved).await, &saved);
            assert_unknown_submission(owner.submit(&saved.request).await, &saved);
            assert_eq!(fixture.provider_counts().requests, accepted + 2);
            assert_eq!(fixture.provider_counts().mints, 1);
            progress("owner_opened", batch, accepted, fixture.provider_counts());
        };
        assert!(accepted > 0 && accepted + 2 < 1024);
        let retained = owner.status(&saved).await;
        eprintln!("unknown_capacity_evidence accepted={accepted} rejected_status={} rejected_error={} retained_status={} original_provider_requests={original_requests} original_mint_requests={original_mints} later_provider_requests={} later_mint_requests={} after_provider_requests={} after_mint_requests={} media_requests={}",
            rejected.0.as_u16(), workflow::closed_error_kind(&rejected), retained.0.as_u16(),
            before_rejection.requests, before_rejection.mints, fixture.provider_counts().requests,
            fixture.provider_counts().mints, fixture.media_requests());
        if retained.0 != StatusCode::OK {
            let closed = owner.close().await;
            let _ = fixture.close().await;
            assert_eq!(retained.0, StatusCode::OK,
                "Unknown status must remain readable after capacity rejection; SQLite owner joined: {}", closed.is_ok());
            return;
        }
        assert_eq!(fixture.provider_counts(), before_rejection, "capacity rejection must precede provider contact");
        assert_unknown_status(retained, &saved);
        assert_closed_capacity(rejected);
        assert_unknown_submission(owner.submit(&saved.request).await, &saved);
        assert_closed_capacity(owner.submit(&request).await);
        workflow::assert_unavailable_audio(owner.audio().await);
        let competing = workflow::unknown_request();
        assert!(competing.operation_id != saved.request.operation_id);
        workflow::assert_conflict(owner.submit(&competing).await);
        assert_unknown_status(owner.status(&saved).await, &saved);
        assert_eq!(fixture.provider_counts(), before_rejection);
        assert_eq!(fixture.media_requests(), 0);
        owner.close().await.expect("the full Unknown owner must join without poisoning retained state");

        let replacement = fixture.open(batch).await;
        assert_unknown_status(replacement.status(&saved).await, &saved);
        assert_unknown_submission(replacement.submit(&saved.request).await, &saved);
        assert_closed_capacity(replacement.submit(&request).await);
        workflow::assert_unavailable_audio(replacement.audio().await);
        workflow::assert_conflict(replacement.submit(&competing).await);
        assert_unknown_status(replacement.status(&saved).await, &saved);
        assert_unknown_submission(replacement.submit(&saved.request).await, &saved);
        assert_eq!(fixture.provider_counts(), before_rejection);
        assert_eq!(fixture.media_requests(), 0);
        replacement.close().await.expect("the replacement full Unknown owner must fully join");

        let (later, media_requests, capacity_events) = fixture.close().await;
        assert_eq!(later.requests, accepted + 2);
        assert_eq!(later.mints, 1);
        assert_eq!(original_requests + later.requests, accepted + 4);
        assert_eq!(original_mints + later.mints, 2);
        assert_eq!(media_requests, 0);
        assert_eq!(capacity_events, 3, "all rejected admissions must report actual numeric SQLITE_FULL");
        eprintln!("unknown_capacity_complete accepted={accepted} original_provider_requests={original_requests} original_mint_requests={original_mints} later_provider_requests={} later_mint_requests={} media_requests={media_requests} capacity_events={capacity_events} elapsed_ms={}",
            later.requests, later.mints, started.elapsed().as_millis());
    }).await.expect("the public Unknown SQLite capacity workflow exceeded its deadline");
}
