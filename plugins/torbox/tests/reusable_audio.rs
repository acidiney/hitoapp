#![cfg(feature = "test-support")]

pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_debrid_torbox::test_support::logs;
pub use hito_debrid_torbox::test_support::owned_tls;
pub use hito_debrid_torbox::test_support::provider;
pub use hito_debrid_torbox::test_support::scripted_media;
pub use hito_testkit::plugin_server as http;
#[path = "support/state_inputs.rs"]
mod state_inputs;
#[path = "support/reusable_workflow.rs"]
pub mod workflow;

use provider::reply::Reply;
use std::time::Duration;
pub use workflow::COOKIE;
use workflow::{SERVICE_TOKEN, Workflow, assert_requests, complete};

#[tokio::test]
async fn two_complete_gets_revalidate_one_ready_candidate_without_another_mint() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let workflow = Workflow::start(
            vec![
                observed(),
                Reply::json(fixtures::acknowledgment()),
                observed(),
                observed(),
            ],
            vec![
                scripted_media::Reply::full(fixtures::wave()),
                scripted_media::Reply::full(fixtures::wave()),
            ],
        )
        .await;
        let saved = workflow.prepare().await;
        complete(workflow.audio().await).await;
        complete(workflow.audio().await).await;
        workflow.assert_ready(&saved).await;
        let (requests, media, capture) = workflow.close().await;
        assert_requests(&requests, 4);
        assert_eq!(media, 2);
        capture.assert_operations(
            &[
                ("candidate_authority", "complete"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "complete"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "complete"),
            ],
            &[
                fixtures::TOKEN,
                SERVICE_TOKEN,
                COOKIE,
                "album/owned.wav",
                &saved.receipt.handle,
            ],
        );
    })
    .await
    .expect("the owned TorBox repeated-GET workflow exceeded its deadline");
}

#[tokio::test]
async fn transient_provider_failure_keeps_the_candidate_and_receipt_for_get_retry() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let mut unavailable = observed();
        unavailable.status = 503;
        let workflow = Workflow::start(
            vec![
                observed(),
                Reply::json(fixtures::acknowledgment()),
                unavailable,
                observed(),
            ],
            vec![scripted_media::Reply::full(fixtures::wave())],
        )
        .await;
        let saved = workflow.prepare().await;
        let failed = workflow.audio().await;
        assert_eq!(failed.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&failed.bytes().await.unwrap());
        assert_eq!(workflow.media.requests(), 0);
        workflow.assert_ready(&saved).await;
        complete(workflow.audio().await).await;
        workflow.assert_ready(&saved).await;
        let (requests, media, capture) = workflow.close().await;
        assert_requests(&requests, 4);
        assert_eq!(media, 1);
        capture.assert_operations(
            &[
                ("candidate_authority", "complete"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "http_rejected"),
                ("bound_audio_opening", "http_rejected"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "complete"),
            ],
            &[
                fixtures::TOKEN,
                SERVICE_TOKEN,
                COOKIE,
                "album/owned.wav",
                &saved.receipt.handle,
            ],
        );
    })
    .await
    .expect("the owned TorBox transient provider recovery exceeded its deadline");
}

#[tokio::test]
async fn one_candidate_holds_two_media_slots_and_recovers_after_a_cancelled_body() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let (first_reply, first_closed) = scripted_media::Reply::held(fixtures::wave());
        let (second_reply, second_closed) = scripted_media::Reply::held(fixtures::wave());
        let workflow = Workflow::start(
            vec![
                observed(),
                Reply::json(fixtures::acknowledgment()),
                observed(),
                observed(),
                observed(),
            ],
            vec![
                first_reply,
                second_reply,
                scripted_media::Reply::full(fixtures::wave()),
            ],
        )
        .await;
        let saved = workflow.prepare().await;
        let mut first = workflow.audio().await;
        let mut second = workflow.audio().await;
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        assert_eq!(second.status(), reqwest::StatusCode::OK);
        assert!(first.chunk().await.unwrap().unwrap().as_ref() == &fixtures::wave()[..44]);
        assert!(second.chunk().await.unwrap().unwrap().as_ref() == &fixtures::wave()[..44]);
        let busy = workflow.audio().await;
        assert_eq!(busy.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&busy.bytes().await.unwrap());
        assert_eq!(workflow.media.requests(), 2);
        workflow.assert_ready(&saved).await;
        drop(first);
        tokio::time::timeout(Duration::from_secs(1), first_closed)
            .await
            .unwrap()
            .unwrap();
        complete(workflow.audio().await).await;
        workflow.assert_ready(&saved).await;
        drop(second);
        tokio::time::timeout(Duration::from_secs(1), second_closed)
            .await
            .unwrap()
            .unwrap();
        let (requests, media, capture) = workflow.close().await;
        assert_requests(&requests, 5);
        assert_eq!(media, 3);
        capture.assert_operations(
            &[
                ("candidate_authority", "complete"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "busy"),
                ("bound_audio_opening", "cancelled"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "cancelled"),
            ],
            &[
                fixtures::TOKEN,
                SERVICE_TOKEN,
                COOKIE,
                "album/owned.wav",
                &saved.receipt.handle,
            ],
        );
    })
    .await
    .expect("the owned TorBox shared-candidate admission control exceeded its deadline");
}

#[tokio::test]
async fn changed_file_retires_readiness_with_its_original_receipt_and_no_media() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let workflow = Workflow::start(
            vec![
                observed(),
                Reply::json(fixtures::acknowledgment()),
                Reply::json(fixtures::observed(true, true, "album/changed.wav")),
            ],
            vec![scripted_media::Reply::full(fixtures::wave())],
        )
        .await;
        let saved = workflow.prepare().await;
        let failed = workflow.audio().await;
        assert_eq!(failed.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&failed.bytes().await.unwrap());
        assert_eq!(workflow.media.requests(), 0);
        let status: hito_plugin_sdk::PreparationStatusResponse = workflow
            .post("v1/sources/preparation/status", &saved.receipt)
            .await;
        status.validate(&saved.receipt).unwrap();
        assert!(matches!(
            status.status,
            hito_plugin_sdk::PreparationStatus::Failed {
                reason: hito_plugin_sdk::PreparationReason::SourceChanged,
            }
        ));
        let replay: hito_plugin_sdk::PreparationSubmitResponse =
            workflow.post("v1/sources/prepare", &saved.request).await;
        replay.validate(&saved.request).unwrap();
        assert!(
            matches!(replay.submission, hito_plugin_sdk::PreparationSubmission::Accepted {
            handle, status: hito_plugin_sdk::PreparationStatus::Failed {
                reason: hito_plugin_sdk::PreparationReason::SourceChanged,
            },
        } if handle == saved.receipt.handle)
        );
        assert!(!workflow.sources().await);
        let repeated = workflow.audio().await;
        assert_eq!(repeated.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&repeated.bytes().await.unwrap());
        assert_eq!(workflow.media.requests(), 0);
        let (requests, media, capture) = workflow.close().await;
        assert_requests(&requests, 3);
        assert_eq!(media, 0);
        capture.assert_operations(
            &[
                ("candidate_authority", "complete"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "source_changed"),
                ("bound_audio_opening", "source_changed"),
                ("bound_audio_opening", "cancelled"),
            ],
            &[
                fixtures::TOKEN,
                SERVICE_TOKEN,
                COOKIE,
                "album/owned.wav",
                "album/changed.wav",
                &saved.receipt.handle,
            ],
        );
    })
    .await
    .expect("the owned TorBox terminal source-change control exceeded its deadline");
}

fn observed() -> Reply {
    Reply::json(fixtures::observed(true, true, "album/owned.wav"))
}
