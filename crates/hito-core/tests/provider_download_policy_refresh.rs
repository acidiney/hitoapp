#[path = "fixtures/content_service.rs"]
mod fixture;
#[path = "fixtures/provider_download_policy_refresh.rs"]
mod policy_fixture;
mod support;

use futures_util::FutureExt;
use hito_core::{DownloadRequest, DownloadState, Error};
use policy_fixture::Library;
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn an_approved_playback_only_manifest_withdraws_queued_work_without_restarting_old_receipts()
{
    let mut library = tokio::time::timeout(Duration::from_secs(10), Library::start())
        .await
        .expect("the owned policy fixture must start within its deadline");
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(queued_case(&mut library)).catch_unwind(),
    )
    .await;
    library.finish().await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the queued policy refresh workflow exceeded its deadline"),
    }
}

#[tokio::test]
async fn a_ready_explicit_copy_survives_approved_playback_only_policy_and_a_stopped_provider() {
    let mut library = tokio::time::timeout(Duration::from_secs(10), Library::start())
        .await
        .expect("the owned policy fixture must start within its deadline");
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(ready_case(&mut library)).catch_unwind(),
    )
    .await;
    library.finish().await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the ready-copy policy refresh workflow exceeded its deadline"),
    }
}

async fn queued_case(library: &mut Library) {
    let recording = library.prepare(None).await;
    assert!(recording.installation.manifest.content_policy.is_none());
    let accepted = library
        .hito
        .downloads
        .create(
            &library.actor,
            &recording.id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let queued = library
        .hito
        .downloads
        .status(&library.actor, &recording.id)
        .await
        .unwrap();
    assert!(matches!(queued.status, DownloadState::Queued));
    assert_eq!(queued.intent_id, accepted.intent_id);
    assert!(queued.source_id.is_none() && queued.bytes_received == 0);
    let preserved = library.private_state(&recording).await;
    library.assert_isolated(&recording).await;
    let updated = library.approve_playback_only(&recording.installation).await;
    assert_eq!(updated.id, recording.installation.id);
    library
        .hito
        .downloads
        .run_next_job_until(pending())
        .await
        .unwrap();
    let withdrawn = library
        .hito
        .downloads
        .status(&library.actor, &recording.id)
        .await
        .unwrap();
    assert!(matches!(withdrawn.status, DownloadState::Failed));
    assert_eq!(withdrawn.intent_id, accepted.intent_id);
    assert!(withdrawn.error.as_deref() == Some("authority_changed"));
    assert!(withdrawn.source_id.is_none() && withdrawn.bytes_received == 0);
    let withdrawn = serde_json::to_value(withdrawn).unwrap();
    let replay = library
        .hito
        .downloads
        .create(
            &library.actor,
            &recording.id,
            DownloadRequest {
                request_id: accepted.request_id,
            },
        )
        .await
        .unwrap();
    assert!(serde_json::to_value(replay).unwrap() == serde_json::to_value(&accepted).unwrap());
    assert!(
        !library
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap(),
        "an exact receipt replay must not restart withdrawn transfer work"
    );
    let rejection = library
        .hito
        .downloads
        .create(
            &library.actor,
            &recording.id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await;
    let message = match rejection {
        Err(Error::Conflict(message)) => message,
        Err(error) => panic!("expected a policy conflict, received {}", error.code()),
        Ok(_) => panic!("new transfer admission must respect the approved playback-only policy"),
    };
    assert!(message.contains("playback only") && message.len() <= 256);
    assert!(!message.contains("private-policy-refresh") && !message.contains("://"));
    assert!(
        !library
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap(),
        "a rejected new operation must not queue transfer work"
    );
    let after = library
        .hito
        .downloads
        .status(&library.actor, &recording.id)
        .await
        .unwrap();
    assert!(
        serde_json::to_value(after).unwrap() == withdrawn,
        "fresh admission rejection must preserve the earlier terminal operation"
    );
    assert!(
        library
            .server
            .as_ref()
            .unwrap()
            .requests("v1/audio")
            .await
            .is_empty()
    );
    let sources = library
        .hito
        .catalog
        .sources(&library.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].id == recording.source && sources[0].ready);
    assert!(
        library.private_state(&recording).await == preserved,
        "withdrawal must preserve catalog identity and private collection state"
    );
    library.assert_isolated(&recording).await;
}

async fn ready_case(library: &mut Library) {
    let recording = library.prepare(Some("explicit_download")).await;
    let accepted = library
        .hito
        .downloads
        .create(
            &library.actor,
            &recording.id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        library
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let ready = library
        .hito
        .downloads
        .status(&library.actor, &recording.id)
        .await
        .unwrap();
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.intent_id, accepted.intent_id);
    assert_eq!(ready.bytes_received, library.original.len() as u64);
    let source = ready
        .source_id
        .clone()
        .expect("a ready copy must expose its local source");
    assert_ne!(source, recording.source);
    let grant = library
        .hito
        .catalog
        .playback_source(&library.actor, &recording.id, &source)
        .await
        .unwrap();
    assert!(grant.is_local());
    let ready = serde_json::to_value(ready).unwrap();
    let preserved = library.private_state(&recording).await;
    library.assert_isolated(&recording).await;
    library.approve_playback_only(&recording.installation).await;
    assert_eq!(
        library
            .server
            .as_ref()
            .unwrap()
            .requests("v1/audio")
            .await
            .len(),
        1
    );
    library.stop_provider().await;

    grant.check().await.unwrap();
    assert!(
        support::audio_bytes(grant.open_file().await.unwrap()) == library.original,
        "the existing local playback grant must retain the exact accepted WAV"
    );
    let reopened = library
        .hito
        .catalog
        .playback_source(&library.actor, &recording.id, &source)
        .await
        .unwrap();
    assert!(reopened.is_local());
    assert!(
        support::audio_bytes(reopened.open_file().await.unwrap()) == library.original,
        "saved playback must open after the actual upstream listener has stopped"
    );
    let replay = library
        .hito
        .downloads
        .create(
            &library.actor,
            &recording.id,
            DownloadRequest {
                request_id: accepted.request_id,
            },
        )
        .await
        .unwrap();
    assert!(serde_json::to_value(replay).unwrap() == serde_json::to_value(&accepted).unwrap());
    let reuse_request = Uuid::new_v4();
    let reused = library
        .hito
        .downloads
        .create(
            &library.actor,
            &recording.id,
            DownloadRequest {
                request_id: reuse_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(reused.request_id, reuse_request);
    assert_eq!(reused.intent_id, accepted.intent_id);
    assert_eq!(reused.recording_id, accepted.recording_id);
    assert!(
        !library
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap(),
        "receipt replay and ready-copy reuse must not create another transfer"
    );
    let after = library
        .hito
        .downloads
        .status(&library.actor, &recording.id)
        .await
        .unwrap();
    assert!(serde_json::to_value(after).unwrap() == ready);
    assert!(
        library.private_state(&recording).await == preserved,
        "tightening policy and stopping upstream must preserve catalog and private state"
    );
    library.assert_isolated(&recording).await;
    assert!(matches!(
        library
            .hito
            .catalog
            .audio_file(&library.other, &source)
            .await,
        Err(Error::NotFound)
    ));
}
