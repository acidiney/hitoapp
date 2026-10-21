#![cfg(feature = "test-support")]

#[path = "support/restart_capacity_entries.rs"]
mod entries;
#[path = "../../hito-debrid-torbox/tests/audio/fixtures.rs"]
pub mod fixtures;
#[path = "../../hito-plugin-all-debrid/tests/fixtures/http.rs"]
pub mod http;
#[path = "../../hito-debrid-torbox/tests/audio/owned_tls.rs"]
pub mod owned_tls;
#[path = "support/restart_capacity_provider.rs"]
mod provider;
#[path = "../../hito-debrid-torbox/tests/audio/scripted_media.rs"]
pub mod scripted_media;
#[path = "support/restart_capacity_workflow.rs"]
mod workflow;

use reqwest::StatusCode;
use std::time::Duration;
use workflow::{Fixture, assert_closed_capacity, assert_ready_replay, assert_ready_status};

pub const COOKIE: &str = "owned-private-torbox-capacity-cookie";

#[tokio::test]
async fn sqlite_capacity_rejects_new_admission_and_retains_ready_audio_after_owner_replacement() {
    tokio::time::timeout(Duration::from_secs(300), async {
        let started = tokio::time::Instant::now();
        let progress = |stage: &str, batch: usize, accepted: usize, counts: provider::Counts| {
            eprintln!("capacity_progress stage={stage} batch={batch} accepted={accepted} provider_requests={} mint_requests={} elapsed_ms={}",
                counts.requests, counts.mints, started.elapsed().as_millis());
        };
        progress("fixture_starting", 0, 0, provider::Counts::default());
        let fixture = Fixture::start().await;
        progress("fixture_started", 0, 0, fixture.provider_counts());
        let mut batch = 0;
        progress("owner_opening", batch, 0, fixture.provider_counts());
        let mut owner = fixture.open(batch).await;
        progress("owner_opened", batch, 0, fixture.provider_counts());
        progress("initial_ready_starting", batch, 0, fixture.provider_counts());
        let saved = owner.prepare_ready().await;
        progress("initial_ready", batch, 0, fixture.provider_counts());
        let mut accepted = 0;
        let (request, rejected, before_rejection) = 'fill: loop {
            for index in entries::batch(batch) {
                let request = entries::request(index);
                let before = fixture.provider_counts();
                let response = owner.submit(&request).await;
                if response.0 != StatusCode::OK {
                    break 'fill (request, response, before);
                }
                workflow::assert_preparing(response, &request);
                accepted += 1;
                assert_eq!(fixture.provider_counts().requests, before.requests + 1);
                assert_eq!(fixture.provider_counts().mints, 1);
                if accepted % 64 == 0 {
                    progress("filling", batch, accepted, fixture.provider_counts());
                }
            }
            assert!(accepted + 1 < 1024, "the test must reach physical SQLite capacity before the receipt-count guard");
            progress("owner_closing", batch, accepted, fixture.provider_counts());
            owner.close().await.expect("each full batch must join its real HTTP and SQLite owners");
            progress("owner_closed", batch, accepted, fixture.provider_counts());
            batch += 1;
            progress("owner_opening", batch, accepted, fixture.provider_counts());
            owner = fixture.open(batch).await;
            progress("owner_opened", batch, accepted, fixture.provider_counts());
        };
        assert!(accepted > 0 && accepted + 1 < 1024);
        let retained = owner.status(&saved).await;
        eprintln!("capacity_evidence accepted={accepted} rejected_status={} rejected_error={} retained_status={} provider_requests={} mint_requests={}",
            rejected.0.as_u16(), workflow::closed_error_kind(&rejected), retained.0.as_u16(), before_rejection.requests, before_rejection.mints);
        assert_eq!(fixture.provider_counts(), before_rejection, "the rejected admission must precede provider contact");

        if retained.0 != StatusCode::OK {
            let closed = owner.close().await;
            let _ = fixture.close().await;
            assert_eq!(retained.0, StatusCode::OK,
                "existing Ready status must remain readable after capacity rejection; SQLite owner joined: {}", closed.is_ok());
            return;
        }
        assert_ready_status(retained, &saved);
        assert_ready_replay(owner.submit(&saved.request).await, &saved);
        assert_closed_capacity(rejected);
        assert_closed_capacity(owner.submit(&request).await);
        assert_eq!(fixture.provider_counts(), before_rejection);

        let mut windows = Vec::new();
        let started = tokio::time::Instant::now();
        workflow::complete(owner.audio().await).await;
        windows.push((started, tokio::time::Instant::now()));
        assert_ready_status(owner.status(&saved).await, &saved);
        owner.close().await.expect("the full live owner must join without poisoning retained state");

        let replacement = fixture.open(batch).await;
        assert_ready_status(replacement.status(&saved).await, &saved);
        assert_ready_replay(replacement.submit(&saved.request).await, &saved);
        let before_replacement_rejection = fixture.provider_counts();
        assert_closed_capacity(replacement.submit(&request).await);
        assert_eq!(fixture.provider_counts(), before_replacement_rejection);
        let started = tokio::time::Instant::now();
        workflow::complete(replacement.audio().await).await;
        windows.push((started, tokio::time::Instant::now()));
        assert_ready_status(replacement.status(&saved).await, &saved);
        assert_ready_replay(replacement.submit(&saved.request).await, &saved);
        replacement.close().await.expect("the replacement full owner must join unchanged");

        let (transcript, media_requests, capacity_events) = fixture.close().await;
        transcript.assert_ready_retention(accepted, &windows);
        assert_eq!(media_requests, 2);
        assert_eq!(capacity_events, 3, "each rejected admission must report an actual numeric SQLITE_FULL result");
    }).await.expect("the public SQLite capacity workflow exceeded its deadline");
}
