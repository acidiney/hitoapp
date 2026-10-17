#[path = "fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use audio_fixture::AudioLibrary;
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    Action, AdoptProviderRecording, DownloadRequest, Error, PlaylistDownloadEntryState as State,
    PlaylistDownloadReceipt, PlaylistDownloadStatus,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

struct Recording {
    id: String,
    entry: Uuid,
    remote_source: &'static str,
}

#[tokio::test]
async fn an_accepted_playlist_retry_waits_for_one_source_without_blocking_other_recordings() {
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
    assert!(std::fs::read(library.music.join("original.wav")).unwrap() == library.original);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(library: &AudioLibrary) {
    let hito = &library.hito;
    let actor = &library.actor;
    let recordings = recordings(library).await;
    assert!(recordings[0].id < recordings[1].id);
    let playlist_id = Uuid::new_v4();
    let playlist = playlist_id.to_string();
    let mut actions = vec![Action::PlaylistCreate {
        playlist_id,
        name: "Owned waiting retry playlist".into(),
    }];
    for (position, recording) in recordings.iter().enumerate() {
        let recording_id = Uuid::parse_str(&recording.id).unwrap();
        actions.extend([
            Action::EntryInsert {
                playlist_id,
                entry_id: recording.entry,
                recording_id,
                position: position as i64,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: format!("My retained title {position}"),
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
        ]);
    }
    support::synchronize(hito, actor, actions).await;
    let before = preserved_state(library, &recordings, &playlist).await;
    assert_no_individual_downloads(library, &recordings).await;
    let original_request = Uuid::new_v4();
    let original_receipt = download(library, &playlist, original_request).await;
    let failed = advance_until(library, &playlist, &recordings, [false, false], [3, 3], 15).await;
    assert_eq!(failed.retention_id, original_receipt.retention_id);
    for entry in &failed.entries.items {
        assert_eq!(entry.status, State::Failed);
        assert_eq!(entry.error.as_deref(), Some("transfer_failed"));
    }

    library
        .server
        .reply("v1/sources", json!({"items":[]}))
        .await;
    let withdrawn = hito
        .catalog
        .refresh_sources(actor, &recordings[0].id)
        .await
        .unwrap();
    assert!(withdrawn.is_empty());
    let available = hito
        .catalog
        .sources(actor, &recordings[1].id)
        .await
        .unwrap();
    assert!(available.iter().any(|source| source.ready));
    library.ready().await;
    let retry_request = Uuid::new_v4();
    let retry_receipt = download(library, &playlist, retry_request).await;
    assert_eq!(retry_receipt.retention_id, original_receipt.retention_id);
    let partial = advance_until(library, &playlist, &recordings, [false, true], [3, 4], 10).await;
    assert_eq!(partial.retention_id, retry_receipt.retention_id);
    assert_eq!(attempts(library, &recordings).await, [3, 4]);
    let waiting = partial
        .entries
        .items
        .iter()
        .find(|entry| entry.recording_id.as_deref() == Some(recordings[0].id.as_str()))
        .unwrap();
    assert_eq!(waiting.status, State::WaitingForSource);
    assert!(waiting.error.is_none());

    library
        .server
        .reply("v1/sources", source_page(recordings[0].remote_source))
        .await;
    let restored = hito
        .catalog
        .refresh_sources(actor, &recordings[0].id)
        .await
        .unwrap();
    assert!(restored.iter().any(|source| source.ready));
    let ready = advance_until(library, &playlist, &recordings, [true, true], [4, 4], 35).await;
    assert_eq!(ready.retention_id, retry_receipt.retention_id);
    assert_eq!(ready.desired_revision, failed.desired_revision);
    let sources: Vec<&str> = ready
        .entries
        .items
        .iter()
        .map(|entry| entry.source_id.as_deref().unwrap())
        .collect();
    assert_ne!(sources[0], sources[1]);
    for source in sources {
        let audio = hito.catalog.audio_file(actor, source).await.unwrap();
        assert!(support::audio_bytes(audio) == library.original);
    }
    for (request_id, receipt) in [
        (original_request, original_receipt),
        (retry_request, retry_receipt),
    ] {
        let replay = download(library, &playlist, request_id).await;
        assert!(serde_json::to_value(replay).unwrap() == serde_json::to_value(receipt).unwrap());
    }
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert_eq!(attempts(library, &recordings).await, [4, 4]);
    assert_no_individual_downloads(library, &recordings).await;
    assert!(preserved_state(library, &recordings, &playlist).await == before);
}

async fn recordings(library: &AudioLibrary) -> [Recording; 2] {
    let second_source = "audio:/2?private=value&other=bad";
    library
        .server
        .reply(
            "v1/recordings/detail",
            json!({
                "id":"second-recording", "title":"Second fixture track", "artist":"Fixture artist"
            }),
        )
        .await;
    library
        .server
        .reply("v1/sources", source_page(second_source))
        .await;
    let second = library
        .hito
        .catalog
        .adopt_provider(
            &library.actor,
            &library.installation.id,
            AdoptProviderRecording {
                installation_revision: library.installation.revision,
                provider_recording_id: "second-recording".into(),
            },
        )
        .await
        .unwrap();
    let mut recordings = [
        Recording {
            id: library.source.recording_id.clone(),
            entry: Uuid::new_v4(),
            remote_source: "audio:/1?private=value&other=bad",
        },
        Recording {
            id: second.id,
            entry: Uuid::new_v4(),
            remote_source: second_source,
        },
    ];
    recordings.sort_by(|left, right| left.id.cmp(&right.id));
    recordings
}

fn source_page(source: &str) -> Value {
    json!({"items":[{"id":source,"label":"Original","ready":true}]})
}

async fn download(
    library: &AudioLibrary,
    playlist: &str,
    request_id: Uuid,
) -> PlaylistDownloadReceipt {
    library
        .hito
        .downloads
        .create_playlist(&library.actor, playlist, DownloadRequest { request_id })
        .await
        .unwrap()
}

async fn attempts(library: &AudioLibrary, recordings: &[Recording; 2]) -> [usize; 2] {
    let requests = library.server.requests("v1/audio").await;
    std::array::from_fn(|index| {
        requests
            .iter()
            .filter(|(_, request)| request["source_id"] == recordings[index].remote_source)
            .count()
    })
}

async fn advance_until(
    library: &AudioLibrary,
    playlist: &str,
    recordings: &[Recording; 2],
    expected_ready: [bool; 2],
    expected_attempts: [usize; 2],
    seconds: u64,
) -> PlaylistDownloadStatus {
    let downloads = &library.hito.downloads;
    tokio::time::timeout(Duration::from_secs(seconds), async {
        loop {
            downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap();
            downloads.run_next_job_until(pending()).await.unwrap();
            let status = downloads
                .playlist_status(&library.actor, playlist, Default::default())
                .await
                .unwrap();
            let counts = attempts(library, recordings).await;
            assert!(
                counts
                    .iter()
                    .zip(expected_attempts)
                    .all(|(count, limit)| *count <= limit),
                "a recording exceeded its provider attempt budget"
            );
            if counts == expected_attempts
                && status.desired_revision == status.reconciled_revision
                && status.entries.next_cursor.is_none()
                && status.entries.items.len() == recordings.len()
                && recordings.iter().enumerate().all(|(index, recording)| {
                    status.entries.items.iter().any(|entry| {
                        entry.entry_id == recording.entry.to_string()
                            && entry.recording_id.as_deref() == Some(recording.id.as_str())
                            && if expected_ready[index] {
                                entry.status == State::Ready
                                    && entry.source_id.is_some()
                                    && entry.error.is_none()
                            } else {
                                matches!(entry.status, State::Failed | State::WaitingForSource)
                                    && entry.source_id.is_none()
                            }
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
        panic!(
            "accepted retry must reach ready={expected_ready:?} with attempts={expected_attempts:?}"
        )
    })
}

async fn assert_no_individual_downloads(library: &AudioLibrary, recordings: &[Recording; 2]) {
    let downloads = &library.hito.downloads;
    for recording in recordings {
        assert!(matches!(
            downloads.status(&library.actor, &recording.id).await,
            Err(Error::NotFound)
        ));
    }
}

async fn preserved_state(
    library: &AudioLibrary,
    recordings: &[Recording; 2],
    playlist: &str,
) -> Value {
    let hito = &library.hito;
    let actor = &library.actor;
    let mut metadata = Vec::new();
    for recording in recordings {
        let detail = hito
            .catalog
            .recording_metadata(actor, &recording.id, Default::default())
            .await
            .unwrap();
        metadata
            .push(json!({"recording":recording.id,"title":detail.title,"artist":detail.artist}));
    }
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "libraries": hito.catalog.libraries(actor).await.unwrap(),
        "playlist": hito.collection.playlist(actor, playlist).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "metadata": metadata
    })
}
