use hito_core::test_support::content_service as fixture;
#[path = "../fixtures/provider_download_policy_refresh.rs"]
mod policy_fixture;
use hito_core::test_support::hang_guard::hang_guard;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{DownloadRequest, DownloadState, Error};
use policy_fixture::Library;
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn an_approved_playback_only_manifest_withdraws_queued_work_without_restarting_old_receipts()
{
    let mut library = hang_guard("the owned policy fixture must start", Library::start()).await;
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
    let mut library = hang_guard("the owned policy fixture must start", Library::start()).await;
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

#[tokio::test]
async fn an_approved_playback_only_manifest_retires_a_partial_download_before_its_body_is_released()
{
    let library = hang_guard("the owned policy fixture must start", Library::start()).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(active_case(&library)).catch_unwind(),
    )
    .await;
    library.finish().await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the active-body policy refresh workflow exceeded its deadline"),
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

async fn active_case(library: &Library) {
    let recording = library.prepare(Some("explicit_download")).await;
    let server = library.server.as_ref().unwrap();
    let tail = server.hold_audio_tail(64);
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
    let preserved = library.private_state(&recording).await;
    library.assert_isolated(&recording).await;
    let downloads = library.hito.downloads.clone();
    let mut worker = Box::pin(downloads.run_next_job_until(pending()));
    tokio::select! {
        result = &mut worker => panic!("the download finished before its held WAV tail: {}", result.is_ok()),
        entered = tokio::time::timeout(Duration::from_secs(3), tail.entered()) => {
            entered.expect("the real provider body must reach its held WAV tail");
        },
    }
    let partial = tokio::select! {
        result = &mut worker => panic!("the download finished before saving partial progress: {}", result.is_ok()),
        partial = tokio::time::timeout(Duration::from_secs(3), async {
            let mut checks = tokio::time::interval(Duration::from_millis(50));
            loop {
                checks.tick().await;
                let status = library.hito.downloads.status(&library.actor, &recording.id).await.unwrap();
                assert!(matches!(status.status, DownloadState::Running));
                if status.bytes_received > 0 { return status; }
            }
        }) => partial.expect("the worker must save partial bytes while the healthy WAV tail remains held"),
    };
    assert_eq!(partial.intent_id, accepted.intent_id);
    assert_eq!(partial.bytes_received, 64);
    assert_eq!(partial.total_bytes, Some(library.original.len() as u64));
    assert!(partial.source_id.is_none() && partial.error.is_none());
    library.assert_managed_partial(partial.bytes_received);
    assert!(!tail.released());
    assert_eq!(server.requests("v1/audio").await.len(), 1);

    let (updated, retired) = tokio::join!(
        library.approve_playback_only(&recording.installation),
        tokio::time::timeout(Duration::from_secs(3), &mut worker),
    );
    assert_eq!(updated.id, recording.installation.id);
    assert!(
        retired
            .expect(
                "approved policy tightening must retire the worker while its WAV tail remains held"
            )
            .unwrap()
    );
    drop(worker);
    tokio::time::timeout(Duration::from_secs(3), tail.dropped())
        .await
        .expect("retirement must drop the actual upstream body without releasing its tail");
    assert!(!tail.released());
    library.assert_no_managed_files();

    let failed = library
        .hito
        .downloads
        .status(&library.actor, &recording.id)
        .await
        .unwrap();
    assert!(matches!(failed.status, DownloadState::Failed));
    assert_eq!(failed.intent_id, accepted.intent_id);
    assert_eq!(failed.recording_id, recording.id);
    assert_eq!(failed.error.as_deref(), Some("authority_changed"));
    assert_eq!(failed.bytes_received, partial.bytes_received);
    assert_eq!(failed.total_bytes, partial.total_bytes);
    assert!(failed.source_id.is_none());
    let terminal = serde_json::to_value(failed).unwrap();
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
        "receipt replay must not restart the retired transfer"
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
        Ok(_) => panic!("approved playback-only policy must deny a fresh persistent transfer"),
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
        "new admission rejection must not queue transfer work"
    );
    let after = library
        .hito
        .downloads
        .status(&library.actor, &recording.id)
        .await
        .unwrap();
    assert!(serde_json::to_value(after).unwrap() == terminal);
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
        "retirement must preserve catalog identity and private collection state"
    );
    library.assert_isolated(&recording).await;
    library.assert_no_managed_files();
    assert!(!tail.released());
    assert_eq!(server.requests("v1/audio").await.len(), 1);
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
        .playback
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
        .playback
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
            .playback
            .audio_file(&library.other, &source)
            .await,
        Err(Error::NotFound)
    ));
}
