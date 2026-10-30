use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::test_support::support;

use audio_fixture::AudioLibrary;
use hito_core::{
    Action, AudioRequest, DownloadRequest, DownloadState, MetadataHistoryQuery, OpenedAudio, Search,
};
use serde_json::{Value, json};
use std::{future::pending, path::PathBuf};
use uuid::Uuid;

#[tokio::test]
async fn a_missing_download_requires_a_fresh_explicit_request_and_preserves_the_old_receipt() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let recording = Uuid::parse_str(&fixture.source.recording_id).unwrap();
    let playlist = Uuid::new_v4();
    support::synchronize(
        &fixture.hito,
        &fixture.actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "My retained recording title".into(),
            },
            Action::LikeSet {
                recording_id: recording,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recording,
                position_ms: 700,
                speed: 1.25,
            },
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "My retained download playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: recording,
                position: 0,
            },
        ],
    )
    .await;
    let before = retained_state(&fixture, playlist).await;
    let original_request = Uuid::new_v4();
    let accepted = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: original_request,
            },
        )
        .await
        .unwrap();
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let ready = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(ready.status, DownloadState::Ready));
    let old_source = ready.source_id.unwrap();
    let owned_media = owned_generated_media(&fixture);
    assert_eq!(std::fs::read(&owned_media).unwrap(), fixture.original);
    std::fs::remove_file(&owned_media).unwrap();
    let transfers = fixture.server.requests("v1/audio").await.len();

    let unavailable = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(unavailable.status, DownloadState::Failed));
    assert_eq!(unavailable.intent_id, accepted.intent_id);
    assert_eq!(unavailable.error.as_deref(), Some("file_unavailable"));
    assert!(unavailable.source_id.is_none());
    assert!(
        fixture
            .hito
            .playback
            .open_audio(&fixture.actor, &old_source, AudioRequest::default())
            .await
            .is_err()
    );
    assert!(
        fixture
            .hito
            .catalog
            .sources(&fixture.actor, &fixture.source.recording_id)
            .await
            .unwrap()
            .iter()
            .all(|source| source.id != old_source)
    );
    fixture
        .hito
        .downloads
        .run_next_job_until(pending())
        .await
        .unwrap();
    assert_eq!(fixture.server.requests("v1/audio").await.len(), transfers);
    let replay = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: original_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.intent_id, accepted.intent_id);
    assert!(matches!(
        fixture
            .hito
            .downloads
            .status(&fixture.actor, &fixture.source.recording_id)
            .await
            .unwrap()
            .status,
        DownloadState::Failed
    ));

    let fresh = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_ne!(fresh.intent_id, accepted.intent_id);
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let restored = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(restored.status, DownloadState::Ready));
    assert_eq!(restored.intent_id, fresh.intent_id);
    let fresh_source = restored.source_id.unwrap();
    assert_ne!(fresh_source, old_source);
    assert_eq!(read_audio(&fixture, &fresh_source).await, fixture.original);
    assert_eq!(
        fixture.server.requests("v1/audio").await.len(),
        transfers + 1
    );

    let replay = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: original_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.intent_id, accepted.intent_id);
    fixture
        .hito
        .downloads
        .remove(&fixture.actor, &replay.intent_id)
        .await
        .unwrap();
    let retained = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(retained.status, DownloadState::Ready));
    assert_eq!(retained.intent_id, fresh.intent_id);
    assert_eq!(retained.source_id.as_deref(), Some(fresh_source.as_str()));
    assert_eq!(read_audio(&fixture, &fresh_source).await, fixture.original);
    assert_eq!(
        fixture.server.requests("v1/audio").await.len(),
        transfers + 1
    );
    assert_eq!(retained_state(&fixture, playlist).await, before);
    assert_eq!(
        std::fs::read(fixture.music.join("original.wav")).unwrap(),
        fixture.original
    );
}

#[tokio::test]
async fn a_checksum_mismatch_fails_the_retained_copy_without_starting_an_implicit_transfer() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let request = Uuid::new_v4();
    let accepted = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: request,
            },
        )
        .await
        .unwrap();
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let ready = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(ready.status, DownloadState::Ready));
    let source = ready.source_id.unwrap();
    let owned_media = owned_generated_media(&fixture);
    let mut changed = fixture.original.clone();
    changed[44] ^= 1;
    std::fs::write(owned_media, &changed).unwrap();
    assert_eq!(changed.len(), fixture.original.len());
    assert_ne!(changed, fixture.original);
    let transfers = fixture.server.requests("v1/audio").await.len();

    assert!(
        fixture
            .hito
            .playback
            .open_audio(&fixture.actor, &source, AudioRequest::default())
            .await
            .is_err()
    );
    let failed = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(failed.status, DownloadState::Failed));
    assert_eq!(failed.intent_id, accepted.intent_id);
    assert_eq!(failed.error.as_deref(), Some("file_unavailable"));
    assert!(failed.source_id.is_none());
    fixture
        .hito
        .downloads
        .run_next_job_until(pending())
        .await
        .unwrap();
    assert_eq!(fixture.server.requests("v1/audio").await.len(), transfers);
    let replay = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.intent_id, accepted.intent_id);
    assert!(matches!(
        fixture
            .hito
            .downloads
            .status(&fixture.actor, &fixture.source.recording_id)
            .await
            .unwrap()
            .status,
        DownloadState::Failed
    ));

    let fresh = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_ne!(fresh.intent_id, accepted.intent_id);
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let repaired = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(repaired.status, DownloadState::Ready));
    assert_eq!(repaired.intent_id, fresh.intent_id);
    let repaired_source = repaired.source_id.unwrap();
    assert_ne!(repaired_source, source);
    assert_eq!(
        read_audio(&fixture, &repaired_source).await,
        fixture.original
    );
    assert_eq!(
        fixture.server.requests("v1/audio").await.len(),
        transfers + 1
    );
    assert_eq!(
        std::fs::read(fixture.music.join("original.wav")).unwrap(),
        fixture.original
    );
}

fn owned_generated_media(fixture: &AudioLibrary) -> PathBuf {
    let owned = fixture.directory.path().join("hito.downloads");
    assert_eq!(owned.parent(), Some(fixture.directory.path()));
    let media: Vec<_> = std::fs::read_dir(&owned)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "media")
        })
        .collect();
    assert_eq!(media.len(), 1);
    let media = &media[0];
    assert!(media.file_type().unwrap().is_file());
    let name = media.file_name().into_string().unwrap();
    let parts: Vec<_> = name.split('.').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[2], "media");
    for part in &parts[..2] {
        assert_eq!(Uuid::parse_str(part).unwrap().to_string(), *part);
    }
    let path = media.path();
    assert_eq!(path.parent(), Some(owned.as_path()));
    path
}

async fn read_audio(fixture: &AudioLibrary, source: &str) -> Vec<u8> {
    let OpenedAudio::Content(mut audio) = fixture
        .hito
        .playback
        .open_audio(&fixture.actor, source, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("retained verified audio must open");
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    bytes
}

async fn retained_state(fixture: &AudioLibrary, playlist: Uuid) -> Value {
    let mut recordings = serde_json::to_value(
        fixture
            .hito
            .catalog
            .recordings(&fixture.actor, Search::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let mut entries = serde_json::to_value(
        fixture
            .hito
            .collection
            .playlist(&fixture.actor, &playlist.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let mut activity = serde_json::to_value(
        fixture
            .hito
            .collection
            .activity(&fixture.actor)
            .await
            .unwrap(),
    )
    .unwrap();
    for rows in [
        &mut recordings["items"],
        &mut entries["entries"],
        &mut activity["history"],
    ] {
        for row in rows.as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("available");
            row.as_object_mut().unwrap().remove("plays_from_services");
        }
    }
    let metadata = fixture
        .hito
        .catalog
        .recording_metadata(
            &fixture.actor,
            &fixture.source.recording_id,
            MetadataHistoryQuery::default(),
        )
        .await
        .unwrap();
    json!({"recordings": recordings, "playlist": entries, "activity": activity, "metadata": metadata})
}
