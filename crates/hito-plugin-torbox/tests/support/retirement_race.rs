use super::{
    COOKIE, ConcurrentProvider, Exchange, PreparationStatus, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, Reply, SERVICE_TOKEN, StatusCode,
    audio_request, completed, fixtures, logs, post, prepare, scripted_media, start, workflow,
};
use hito_plugin_sdk::PreparationReason;
use std::time::Duration;

#[tokio::test]
async fn later_source_change_upgrades_same_operation_retired_by_an_overlapping_get() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let (incomplete, incomplete_received, incomplete_release) = Reply::paused(fixtures::observed(false, true, "album/owned.wav"));
        let (changed, changed_received, changed_release) = Reply::paused(fixtures::observed(true, true, "album/changed.wav"));
        let provider = ConcurrentProvider::start(vec![completed(), Exchange::Mint(Reply::json(fixtures::acknowledgment())), Exchange::Observe(incomplete), Exchange::Observe(changed)]).await;
        let media = scripted_media::ScriptedMedia::start(vec![scripted_media::Reply::full(fixtures::wave())]).await;
        let capture = logs::Capture::default();
        let (service, client) = start(&provider, &media, capture.dispatch()).await;
        let (request, receipt) = prepare(&client, &service).await;
        let first = audio_request(&client, &service);
        let first = tokio::spawn(async move { first.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), incomplete_received).await.unwrap().unwrap();
        let second = audio_request(&client, &service);
        let second = tokio::spawn(async move { second.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), changed_received).await.unwrap().unwrap();
        assert_eq!(provider.requests(), (4, 1));
        assert_eq!(media.requests(), 0);
        incomplete_release.send(()).unwrap();
        let first = first.await.unwrap();
        assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&first.bytes().await.unwrap());
        let status: PreparationStatusResponse = post(&client, &service, "v1/sources/preparation/status", &receipt).await;
        status.validate(&receipt).unwrap();
        assert!(matches!(status.status, PreparationStatus::Failed { reason: PreparationReason::SourceUnavailable }));
        changed_release.send(()).unwrap();
        let second = second.await.unwrap();
        assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&second.bytes().await.unwrap());
        let status: PreparationStatusResponse = post(&client, &service, "v1/sources/preparation/status", &receipt).await;
        status.validate(&receipt).unwrap();
        assert!(matches!(status.status, PreparationStatus::Failed { reason: PreparationReason::SourceChanged }), "a later definite source-change witness must upgrade the same retired operation");
        let replay: PreparationSubmitResponse = post(&client, &service, "v1/sources/prepare", &request).await;
        replay.validate(&request).unwrap();
        assert!(matches!(replay.submission, PreparationSubmission::Accepted { handle, status: PreparationStatus::Failed { reason: PreparationReason::SourceChanged } } if handle == receipt.handle));
        let page: hito_plugin_sdk::SourcePage = post(&client, &service, "v1/sources", &serde_json::json!({"recording_id":request.recording_id})).await;
        page.validate().unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(!page.items[0].ready);
        assert_eq!(provider.requests(), (4, 1));
        assert_eq!(media.requests(), 0);
        service.close().await.unwrap();
        assert_eq!(provider.close().await, (4, 1));
        assert_eq!(media.close().await, 0);
        capture.assert_operations(&[
            ("torrent_observation", "provider_completed"), ("bound_file_preparation", "candidate"),
            ("torrent_observation", "incomplete"), ("bound_audio_opening", "incomplete"),
            ("torrent_observation", "source_changed"), ("bound_audio_opening", "source_changed"),
        ], &[fixtures::TOKEN, SERVICE_TOKEN, COOKIE, "album/owned.wav", "album/changed.wav", &receipt.handle]);
    }).await.expect("the owned concurrent TorBox retirement control exceeded its deadline");
}

#[tokio::test]
async fn an_older_source_change_cannot_retire_a_newer_ready_operation() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let (incomplete, incomplete_received, incomplete_release) = Reply::paused(fixtures::observed(false, true, "album/owned.wav"));
        let (changed, changed_received, changed_release) = Reply::paused(fixtures::observed(true, true, "album/changed.wav"));
        let provider = ConcurrentProvider::start(vec![
            completed(), Exchange::Mint(Reply::json(fixtures::acknowledgment())),
            Exchange::Observe(incomplete), Exchange::Observe(changed),
            completed(), Exchange::Mint(Reply::json(fixtures::acknowledgment())), completed(),
        ]).await;
        let media = scripted_media::ScriptedMedia::start(vec![scripted_media::Reply::full(fixtures::wave())]).await;
        let capture = logs::Capture::default();
        let (service, client) = start(&provider, &media, capture.dispatch()).await;
        let (old_request, old_receipt) = prepare(&client, &service).await;
        let first = audio_request(&client, &service);
        let first = tokio::spawn(async move { first.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), incomplete_received).await.unwrap().unwrap();
        let second = audio_request(&client, &service);
        let second = tokio::spawn(async move { second.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), changed_received).await.unwrap().unwrap();
        incomplete_release.send(()).unwrap();
        let first = first.await.unwrap();
        assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&first.bytes().await.unwrap());
        let old: PreparationStatusResponse = post(&client, &service, "v1/sources/preparation/status", &old_receipt).await;
        old.validate(&old_receipt).unwrap();
        assert!(matches!(old.status, PreparationStatus::Failed { reason: PreparationReason::SourceUnavailable }));
        let (new_request, new_receipt) = prepare(&client, &service).await;
        assert_ne!(old_request.operation_id, new_request.operation_id);
        assert!(old_receipt.handle != new_receipt.handle);
        assert_eq!(provider.requests(), (6, 2));
        assert_eq!(media.requests(), 0);
        changed_release.send(()).unwrap();
        let second = second.await.unwrap();
        assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&second.bytes().await.unwrap());
        let old: PreparationStatusResponse = post(&client, &service, "v1/sources/preparation/status", &old_receipt).await;
        old.validate(&old_receipt).unwrap();
        assert!(matches!(old.status, PreparationStatus::Failed { reason: PreparationReason::SourceUnavailable }));
        let new: PreparationStatusResponse = post(&client, &service, "v1/sources/preparation/status", &new_receipt).await;
        new.validate(&new_receipt).unwrap();
        assert!(matches!(new.status, PreparationStatus::Ready {}));
        let replay: PreparationSubmitResponse = post(&client, &service, "v1/sources/prepare", &new_request).await;
        replay.validate(&new_request).unwrap();
        assert!(matches!(replay.submission, PreparationSubmission::Accepted { handle, status: PreparationStatus::Ready {} } if handle == new_receipt.handle));
        workflow::complete(audio_request(&client, &service).send().await.unwrap()).await;
        assert_eq!(provider.requests(), (7, 2));
        assert_eq!(media.requests(), 1);
        service.close().await.unwrap();
        assert_eq!(provider.close().await, (7, 2));
        assert_eq!(media.close().await, 1);
        capture.assert_operations(&[
            ("torrent_observation", "provider_completed"), ("bound_file_preparation", "candidate"),
            ("torrent_observation", "incomplete"), ("bound_audio_opening", "incomplete"),
            ("torrent_observation", "provider_completed"), ("bound_file_preparation", "candidate"),
            ("torrent_observation", "source_changed"), ("bound_audio_opening", "source_changed"),
            ("torrent_observation", "provider_completed"), ("bound_audio_opening", "complete"), ("bound_audio_opening", "complete"),
        ], &[fixtures::TOKEN, SERVICE_TOKEN, COOKIE, "album/owned.wav", "album/changed.wav", &old_receipt.handle, &new_receipt.handle]);
    }).await.expect("the owned TorBox newer-operation isolation exceeded its deadline");
}
