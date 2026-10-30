use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AdoptProviderRecording, DownloadRequest, Error, InstallPlugin,
    OfferPluginService, PlaylistDownloadEntryState, PlaylistDownloadStatus,
    PreviewPluginInstallation, Services,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn a_playback_only_playlist_entry_settles_as_unavailable_without_creating_a_transfer() {
    let (_directory, hito, actor, _library, music) = tokio::time::timeout(
        Duration::from_secs(10),
        support::library("provider-playlist-policy"),
    )
    .await
    .expect("the temporary library must open within its deadline");
    let original_path = music.join("original.wav");
    let original = support::write_wav(&original_path, 423);
    let server = fixture::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(&hito, &actor, &server, &original)).catch_unwind(),
    )
    .await;
    let settled =
        tokio::time::timeout(Duration::from_secs(5), hito.downloads.finish_downloads()).await;
    let joined = tokio::time::timeout(Duration::from_secs(2), server.shutdown()).await;
    assert!(
        matches!(settled, Ok(Ok(()))),
        "managed filesystem work must settle on success or panic"
    );
    assert!(joined.is_ok(), "the owned provider must shut down and join");
    assert!(
        std::fs::read(original_path).unwrap() == original,
        "playlist policy must preserve the user-owned original bytes"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public playlist policy workflow exceeded its deadline"),
    }
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    original: &[u8],
) {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-policy-fixture",
                "name": "Owned playback-only playlist fixture",
                "protocol_version": 1,
                "capabilities": ["sources"],
                "content_policy": {"source_persistence": "playback_only"}
            }),
        )
        .await;
    server
        .reply(
            "v1/recordings/detail",
            json!({
                "id": "private-playlist-policy-recording",
                "title": "private-playlist-policy-title",
                "artist": "private-playlist-policy-artist",
                "duration_ms": 1000
            }),
        )
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{
                "id": "private-playlist-policy-source",
                "label": "Playback",
                "ready": true
            }]}),
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
                name: "Owned playback-only playlist fixture".into(),
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
    let installation = hito
        .plugins
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
        .unwrap();
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: "private-playlist-policy-recording".into(),
            },
        )
        .await
        .unwrap();
    let sources = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].ready && sources[0].provider == "plugin");
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    let playlist_id = Uuid::new_v4();
    let entry_id = Uuid::new_v4();
    let playlist = playlist_id.to_string();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "private-playlist-policy-editable-list".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id,
                recording_id,
                position: 0,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "private-playlist-policy-personal-title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "private-playlist-policy-personal-artist".into(),
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
        ],
    )
    .await;
    let preserved = private_state(hito, actor, &recording.id, &playlist).await;
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
    assert!(server.requests("v1/audio").await.is_empty());
    assert_eq!(terminal.retention_id, accepted.retention_id);
    assert_eq!(terminal.entries.items.len(), 1);
    let entry = &terminal.entries.items[0];
    assert_eq!(entry.entry_id, entry_id.to_string());
    assert_eq!(entry.recording_id.as_deref(), Some(recording.id.as_str()));
    assert_eq!(entry.status, PlaylistDownloadEntryState::Unavailable);
    assert!(
        entry.error.as_deref() == Some("policy_restricted"),
        "the terminal reason must be the safe policy code"
    );
    assert!(entry.source_id.is_none());
    assert!(matches!(
        hito.downloads.status(actor, &recording.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        private_state(hito, actor, &recording.id, &playlist).await == preserved,
        "policy admission must preserve the playlist and private collection state"
    );

    tokio::time::sleep(Duration::from_secs(6)).await;
    for _ in 0..3 {
        assert!(
            !hito
                .downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap(),
            "a terminal policy restriction must leave reconciliation idle"
        );
        assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
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
    assert!(server.requests("v1/audio").await.is_empty());
    assert!(
        private_state(hito, actor, &recording.id, &playlist).await == preserved,
        "later idle turns must preserve the playlist and private collection state"
    );
    let after = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(after.len(), 1);
    assert!(after[0].id == sources[0].id && after[0].ready);
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
            assert!(
                !hito.downloads.run_next_job_until(pending()).await.unwrap(),
                "a restricted playlist entry must not create transfer work"
            );
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
