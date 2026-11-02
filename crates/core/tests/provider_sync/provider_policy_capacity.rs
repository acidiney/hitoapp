use hito_core::test_support::content_service as fixture;
use hito_core::test_support::hang_guard::hang_guard;
use hito_core::test_support::support;

use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AdoptProviderRecording, DownloadReceipt, DownloadRequest, DownloadState, Error,
    InstallPlugin, OfferPluginService, PlaylistDownloadEntryState, PlaylistDownloadStatus,
    PluginInstallation, PreviewPluginInstallation, Recording, Services,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn playback_only_policy_remains_terminal_when_the_accounts_download_capacity_is_full() {
    let (_directory, hito, actor, _library, music) = hang_guard(
        "the temporary library must open",
        support::library("provider-policy-capacity"),
    )
    .await;
    let original_path = music.join("original.wav");
    let original = support::write_wav(&original_path, 425);
    let permitted = fixture::ProviderServer::start().await;
    let restricted = fixture::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(&hito, &actor, &permitted, &restricted, &original))
            .catch_unwind(),
    )
    .await;
    let settled =
        tokio::time::timeout(Duration::from_secs(5), hito.downloads.finish_downloads()).await;
    let permitted_joined = tokio::time::timeout(Duration::from_secs(2), permitted.shutdown()).await;
    let restricted_joined =
        tokio::time::timeout(Duration::from_secs(2), restricted.shutdown()).await;
    assert!(
        matches!(settled, Ok(Ok(()))),
        "managed filesystem work must settle on success or panic"
    );
    assert!(
        permitted_joined.is_ok() && restricted_joined.is_ok(),
        "both owned providers must shut down and join"
    );
    assert!(
        std::fs::read(original_path).unwrap() == original,
        "capacity and policy admission must preserve the user-owned original bytes"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public capacity policy workflow exceeded its deadline"),
    }
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    permitted: &fixture::ProviderServer,
    restricted: &fixture::ProviderServer,
    original: &[u8],
) {
    let permitted_installation =
        connect(hito, actor, permitted, "explicit_download", original).await;
    let restricted_installation = connect(hito, actor, restricted, "playback_only", original).await;
    let mut queued = Vec::with_capacity(32);
    for index in 0..32 {
        let recording = adopt(
            hito,
            actor,
            permitted,
            &permitted_installation,
            &format!("private-capacity-permitted-{index}"),
        )
        .await;
        let receipt = hito
            .downloads
            .create(
                actor,
                &recording.id,
                DownloadRequest {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        queued.push((recording.id, receipt));
    }
    let recording = adopt(
        hito,
        actor,
        restricted,
        &restricted_installation,
        "private-capacity-restricted",
    )
    .await;
    let sources = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].ready && sources[0].provider == "plugin");
    let playlist_id = Uuid::new_v4();
    let entry_id = Uuid::new_v4();
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    let first_queued_id = Uuid::parse_str(&queued[0].0).unwrap();
    let playlist = playlist_id.to_string();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "private-capacity-playlist".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id,
                recording_id,
                position: 0,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "private-capacity-personal-title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "private-capacity-personal-artist".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 700,
                speed: 1.25,
            },
            Action::LikeSet {
                recording_id: first_queued_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: first_queued_id,
                position_ms: 600,
                speed: 1.0,
            },
        ],
    )
    .await;
    let queued_before = queued_state(hito, actor, &queued).await;
    let private_before = private_state(hito, actor, &recording.id, &playlist).await;
    assert!(permitted.requests("v1/audio").await.is_empty());
    assert!(restricted.requests("v1/audio").await.is_empty());

    let rejection = hito
        .downloads
        .create(
            actor,
            &recording.id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await;
    let message = match rejection {
        Err(Error::Conflict(message)) => message,
        Err(error) => panic!("expected a policy conflict, received {}", error.code()),
        Ok(_) => panic!("capacity must not permit a restricted persistent download"),
    };
    assert!(
        message.contains("playback only"),
        "capacity must not replace the selected source policy reason"
    );
    assert!(message.len() <= 256);
    assert!(!message.contains("private-capacity"));
    assert!(!message.contains("://"));
    assert!(matches!(
        hito.downloads.status(actor, &recording.id).await,
        Err(Error::NotFound)
    ));

    let accepted = hito
        .downloads
        .create_playlist(
            actor,
            &playlist,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let terminal = reconcile_until_idle(hito, actor, &playlist).await;
    assert_eq!(terminal.retention_id, accepted.retention_id);
    assert_eq!(terminal.entries.items.len(), 1);
    let entry = &terminal.entries.items[0];
    assert_eq!(entry.entry_id, entry_id.to_string());
    assert_eq!(entry.recording_id.as_deref(), Some(recording.id.as_str()));
    assert_eq!(entry.status, PlaylistDownloadEntryState::Unavailable);
    assert!(entry.error.as_deref() == Some("policy_restricted"));
    assert!(entry.source_id.is_none());

    tokio::time::sleep(Duration::from_secs(6)).await;
    for _ in 0..3 {
        assert!(
            !hito
                .downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap(),
            "a restricted member must not retry when the former capacity delay expires"
        );
        let later = hito
            .downloads
            .playlist_status(actor, &playlist, Default::default())
            .await
            .unwrap();
        assert!(
            later == terminal,
            "the terminal public outcome must remain stable"
        );
    }
    assert!(matches!(
        hito.downloads.status(actor, &recording.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        queued_state(hito, actor, &queued).await == queued_before,
        "all original queued receipts and transfer states must remain unchanged"
    );
    assert!(
        private_state(hito, actor, &recording.id, &playlist).await == private_before,
        "policy admission must preserve the playlist and private collection state"
    );
    assert!(permitted.requests("v1/audio").await.is_empty());
    assert!(restricted.requests("v1/audio").await.is_empty());
    let after = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(after.len(), 1);
    assert!(after[0].id == sources[0].id && after[0].ready);
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    persistence: &str,
    original: &[u8],
) -> PluginInstallation {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": format!("org.hito.capacity-{persistence}"),
                "name": "Owned capacity fixture",
                "protocol_version": 1,
                "capabilities": ["sources"],
                "content_policy": {"source_persistence": persistence}
            }),
        )
        .await;
    server
        .audio(
            StatusCode::OK,
            HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "audio/wav".parse().unwrap(),
            )]),
            original.to_vec(),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned capacity fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let preview = hito
        .plugins
        .preview_installation(
            actor,
            PreviewPluginInstallation {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: Some(preview.digest),
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}

async fn adopt(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    installation: &PluginInstallation,
    remote: &str,
) -> Recording {
    server
        .recording_reply(
            "v1/recordings/detail",
            remote,
            json!({
                "id": remote,
                "title": format!("private-capacity-title-{remote}"),
                "artist": "private-capacity-artist",
                "duration_ms": 1000
            }),
        )
        .await;
    server
        .recording_reply(
            "v1/sources",
            remote,
            json!({"items": [{
                "id": format!("private-capacity-source-{remote}"),
                "label": "Owned audio",
                "ready": true
            }]}),
        )
        .await;
    hito.catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: remote.into(),
            },
        )
        .await
        .unwrap()
}

async fn queued_state(
    hito: &Services,
    actor: &Actor,
    queued: &[(String, DownloadReceipt)],
) -> Value {
    assert_eq!(queued.len(), 32);
    let mut states = Vec::with_capacity(queued.len());
    for (recording, receipt) in queued {
        let replay = hito
            .downloads
            .create(
                actor,
                recording,
                DownloadRequest {
                    request_id: receipt.request_id,
                },
            )
            .await
            .unwrap();
        assert!(serde_json::to_value(&replay).unwrap() == serde_json::to_value(receipt).unwrap());
        let status = hito.downloads.status(actor, recording).await.unwrap();
        assert!(matches!(status.status, DownloadState::Queued));
        assert!(status.intent_id == receipt.intent_id && status.recording_id == *recording);
        assert!(status.source_id.is_none() && status.error.is_none());
        assert!(status.bytes_received == 0 && status.total_bytes.is_none());
        states.push(json!({"receipt": replay, "status": status}));
    }
    Value::Array(states)
}

async fn reconcile_until_idle(
    hito: &Services,
    actor: &Actor,
    playlist: &str,
) -> PlaylistDownloadStatus {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let progressed = hito
                .downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap();
            let status = hito
                .downloads
                .playlist_status(actor, playlist, Default::default())
                .await
                .unwrap();
            if status.desired_revision == status.reconciled_revision && !progressed {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the owned playlist must finish membership reconciliation and drain to idle")
}

async fn private_state(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "playlist": hito.collection.playlist(actor, playlist).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "metadata": {"title": metadata.title, "artist": metadata.artist}
    })
}
