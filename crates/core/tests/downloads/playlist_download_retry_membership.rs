use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::test_support::support;

use audio_fixture::AudioLibrary;
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    Action, DownloadRequest, Error, PlaylistDownloadEntryState, PlaylistDownloadStatus,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn an_accepted_playlist_retry_survives_an_unrelated_membership_revision() {
    let library = AudioLibrary::start().await;
    library
        .server
        .audio(
            StatusCode::SERVICE_UNAVAILABLE,
            HeaderMap::new(),
            Vec::new(),
        )
        .await;
    let outcome = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.hito.downloads.finish_downloads().await.unwrap();
    assert!(
        std::fs::read(library.music.join("original.wav")).unwrap() == library.original,
        "playlist retry must preserve the user-owned original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(library: &AudioLibrary) {
    let hito = &library.hito;
    let actor = &library.actor;
    let recording = &library.source.recording_id;
    let recording_id = Uuid::parse_str(recording).unwrap();
    let playlist_id = Uuid::new_v4();
    let playlist = playlist_id.to_string();
    let copy_id = Uuid::new_v4();
    let first_entry = Uuid::new_v4();
    let duplicate_entry = Uuid::new_v4();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "Owned retry playlist".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: first_entry,
                recording_id,
                position: 0,
            },
            Action::PlaylistCreate {
                playlist_id: copy_id,
                name: "Independent editable copy".into(),
            },
            Action::EntryInsert {
                playlist_id: copy_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "My retained title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My retained artist".into(),
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
    let preserved = preserved_state(library, copy_id).await;
    assert!(preserved["metadata"]["title"]["personal"] == "My retained title");
    assert!(preserved["metadata"]["artist"]["personal"] == "My retained artist");
    assert!(preserved["activity"]["history"][0]["position_ms"] == 700);
    assert!(preserved["activity"]["history"][0]["speed"] == 1.25);
    assert_no_individual_download(library).await;
    assert!(library.server.requests("v1/audio").await.is_empty());

    let original_request = Uuid::new_v4();
    let original_receipt = hito
        .downloads
        .create_playlist(
            actor,
            &playlist,
            DownloadRequest {
                request_id: original_request,
            },
        )
        .await
        .unwrap();
    let failed = advance_until(
        library,
        &playlist,
        &[first_entry],
        PlaylistDownloadEntryState::Failed,
        3,
    )
    .await;
    assert_eq!(failed.retention_id, original_receipt.retention_id);
    assert_eq!(
        failed.entries.items[0].error.as_deref(),
        Some("transfer_failed")
    );
    assert!(failed.entries.items[0].source_id.is_none());
    assert!(preserved_state(library, copy_id).await == preserved);
    assert_no_individual_download(library).await;

    library.ready().await;
    let retry_request = Uuid::new_v4();
    let retry_receipt = hito
        .downloads
        .create_playlist(
            actor,
            &playlist,
            DownloadRequest {
                request_id: retry_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(retry_receipt.retention_id, original_receipt.retention_id);
    support::synchronize(
        hito,
        actor,
        vec![Action::EntryInsert {
            playlist_id,
            entry_id: duplicate_entry,
            recording_id,
            position: 1,
        }],
    )
    .await;
    let changed = hito
        .downloads
        .playlist_status(actor, &playlist, Default::default())
        .await
        .unwrap();
    assert_eq!(changed.retention_id, original_receipt.retention_id);
    assert!(changed.desired_revision > failed.desired_revision);
    assert_eq!(changed.entries.items.len(), 2);
    assert_eq!(library.server.requests("v1/audio").await.len(), 3);
    assert!(preserved_state(library, copy_id).await == preserved);
    let accepted_events =
        serde_json::to_value(hito.collection.events(actor, 0).await.unwrap()).unwrap();
    let changed_playlist =
        serde_json::to_value(hito.collection.playlist(actor, &playlist).await.unwrap()).unwrap();

    let ready = advance_until(
        library,
        &playlist,
        &[first_entry, duplicate_entry],
        PlaylistDownloadEntryState::Ready,
        4,
    )
    .await;
    assert_eq!(ready.retention_id, original_receipt.retention_id);
    assert_eq!(ready.desired_revision, changed.desired_revision);
    let source = ready.entries.items[0].source_id.as_deref().unwrap();
    assert_ne!(source, library.source.id);
    for entry in &ready.entries.items {
        assert_eq!(entry.source_id.as_deref(), Some(source));
        assert!(entry.error.is_none());
    }
    let sources = hito.catalog.sources(actor, recording).await.unwrap();
    assert_eq!(
        sources
            .iter()
            .filter(|item| item.provider == "download")
            .count(),
        1
    );
    let audio = hito.playback.audio_file(actor, source).await.unwrap();
    assert!(support::audio_bytes(audio) == library.original);
    assert_eq!(
        library.server.requests("v1/audio").await.len(),
        4,
        "both occurrences must share one successful replacement after the three original failed attempts"
    );
    for (request_id, receipt) in [
        (original_request, original_receipt),
        (retry_request, retry_receipt),
    ] {
        let replay = hito
            .downloads
            .create_playlist(actor, &playlist, DownloadRequest { request_id })
            .await
            .unwrap();
        assert!(serde_json::to_value(replay).unwrap() == serde_json::to_value(receipt).unwrap());
    }
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert_eq!(library.server.requests("v1/audio").await.len(), 4);
    assert_no_individual_download(library).await;
    assert!(preserved_state(library, copy_id).await == preserved);
    assert!(
        serde_json::to_value(hito.collection.events(actor, 0).await.unwrap()).unwrap()
            == accepted_events
    );
    assert!(
        serde_json::to_value(hito.collection.playlist(actor, &playlist).await.unwrap()).unwrap()
            == changed_playlist
    );
}

async fn advance_until(
    library: &AudioLibrary,
    playlist: &str,
    entries: &[Uuid],
    expected: PlaylistDownloadEntryState,
    expected_attempts: usize,
) -> PlaylistDownloadStatus {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            library
                .hito
                .downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap();
            library
                .hito
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap();
            let status = library
                .hito
                .downloads
                .playlist_status(&library.actor, playlist, Default::default())
                .await
                .unwrap();
            let attempts = library.server.requests("v1/audio").await.len();
            assert!(attempts <= expected_attempts, "playlist retry exceeded its provider attempt budget");
            if attempts == expected_attempts
                && status.desired_revision == status.reconciled_revision
                && status.entries.next_cursor.is_none()
                && status.entries.items.len() == entries.len()
                && entries.iter().all(|entry_id| {
                    status.entries.items.iter().any(|entry| {
                        entry.entry_id == entry_id.to_string()
                            && entry.recording_id.as_deref() == Some(library.source.recording_id.as_str())
                            && entry.status == expected
                    })
                })
            {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("the accepted playlist command must make every live occurrence {expected:?} after {expected_attempts} provider attempts")
    })
}

async fn assert_no_individual_download(library: &AudioLibrary) {
    assert!(
        matches!(
            library
                .hito
                .downloads
                .status(&library.actor, &library.source.recording_id)
                .await,
            Err(Error::NotFound)
        ),
        "playlist retry must not create an individual retention"
    );
}

async fn preserved_state(library: &AudioLibrary, copy: Uuid) -> Value {
    let hito = &library.hito;
    let actor = &library.actor;
    let metadata = hito
        .catalog
        .recording_metadata(actor, &library.source.recording_id, Default::default())
        .await
        .unwrap();
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "libraries": hito.catalog.libraries(actor).await.unwrap(),
        "copy": hito.collection.playlist(actor, &copy.to_string()).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "metadata": {"title": metadata.title, "artist": metadata.artist}
    })
}
