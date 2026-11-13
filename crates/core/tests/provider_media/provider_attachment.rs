use hito_core::test_support::provider_attachment as fixture;
use hito_core::test_support::support;

use axum::http::{HeaderMap, StatusCode};
use hito_core::{
    Action, AdoptProviderRecording, AttachReviewedProviderSource, ImportProviderPlaylist,
    OpenedAudio, PlaylistEntry, ProviderCandidate,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn reviewed_source_attachment_preserves_copied_identity_metadata_and_collection_state() {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library("provider-attachment").await;
    let spotify_installation = fixture::connect(
        &hito,
        &actor,
        &spotify,
        "org.hito.spotify-fixture",
        &["playlists"],
    )
    .await;
    let youtube_installation = fixture::connect(
        &hito,
        &actor,
        &youtube,
        "org.hito.youtube-fixture",
        &["search", "sources"],
    )
    .await;
    let first = json!({
        "id": "spotify/a", "title": "Spotify title", "artist": "Spotify artist",
        "duration_ms": 90_000, "revision": "spotify-r1",
        "release": {"id": "spotify/album", "title": "Spotify album", "artist": "Spotify artist"},
        "track_number": 1
    });
    let second = json!({
        "id": "spotify/b", "title": "Second title", "artist": "Second artist"
    });
    fixture::playlist(
        &spotify,
        "spotify/playlist",
        "spotify-snapshot",
        vec![
            json!({"occurrence_id": "first-a", "item": {"kind": "recording", "recording": first}}),
            json!({"occurrence_id": "b", "item": {"kind": "recording", "recording": second}}),
            json!({"occurrence_id": "second-a", "item": {"kind": "recording", "recording": first}}),
        ],
    )
    .await;
    let receipt = hito
        .collection
        .import_provider_playlist(
            &actor,
            &spotify_installation.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: spotify_installation.revision,
                playlist_reference: "https://provider.invalid/owned-playlist".into(),
            },
        )
        .await
        .unwrap();
    let playlist_id = receipt.playlist_id.to_string();
    let imported = hito
        .collection
        .playlist(&actor, &playlist_id)
        .await
        .unwrap();
    assert_eq!(imported.entries.len(), 3);
    let recording = imported.entries[0].recording_id.clone().unwrap();
    assert_eq!(
        imported.entries[2].recording_id.as_deref(),
        Some(recording.as_str())
    );
    assert_ne!(
        imported.entries[1].recording_id.as_deref(),
        Some(recording.as_str())
    );
    assert!(imported.entries.iter().all(|entry| !entry.available));
    let recording_id = Uuid::parse_str(&recording).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My artist".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4_200,
                speed: 1.25,
            },
        ],
    )
    .await;
    let before = hito
        .catalog
        .recording_metadata(&actor, &recording, Default::default())
        .await
        .unwrap();
    let selected = serde_json::to_value(&before.selected_observation).unwrap();
    assert_eq!(selected["provider"], "org.hito.spotify-fixture");
    assert_eq!(before.title.personal.as_deref(), Some("My title"));
    assert_eq!(before.artist.personal.as_deref(), Some("My artist"));
    assert!(before.title.operation_id.is_some());
    assert!(before.artist.operation_id.is_some());
    let inherited = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.id == recording)
        .unwrap();
    let origin = serde_json::to_value(&imported.origin).unwrap();
    let identities: Vec<Value> = imported.entries.iter().map(entry_identity).collect();
    let history = hito
        .collection
        .activity(&actor)
        .await
        .unwrap()
        .history
        .remove(0);
    hito.plugins
        .remove(
            &actor,
            &spotify_installation.id,
            spotify_installation.revision,
        )
        .await
        .unwrap();
    let original = support::write_wav(&directory.path().join("owned-youtube.wav"), 10);
    youtube
        .reply(
            "v1/recordings/detail",
            json!({
                "id": "youtube/candidate", "title": "Different YouTube title",
                "artist": "Different YouTube artist", "duration_ms": 91_000,
                "revision": "youtube-r1",
                "release": {"id": "youtube/album", "title": "YouTube album", "artist": "YouTube artist"}
            }),
        )
        .await;
    youtube
        .reply(
            "v1/sources",
            json!({"items": [{"id": "youtube/audio", "label": "Owned audio", "ready": true}]}),
        )
        .await;
    youtube
        .audio(
            StatusCode::OK,
            HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "audio/wav".parse().unwrap(),
            )]),
            original.clone(),
        )
        .await;
    let candidate = ProviderCandidate {
        installation_id: youtube_installation.id.clone(),
        installation_revision: youtube_installation.revision,
        provider_recording_id: "youtube/candidate".into(),
    };
    let review = hito
        .catalog
        .review_provider_source(&actor, &recording, candidate.clone())
        .await
        .expect("an owned candidate can be reviewed after the playlist origin is disconnected");
    assert_eq!(review.candidate.title, "Different YouTube title");
    let mut attached = hito
        .catalog
        .attach_provider_source(
            &actor,
            &recording,
            AttachReviewedProviderSource {
                candidate,
                review_stamp: review.review_stamp,
            },
        )
        .await
        .expect("confirmed source attachment keeps the existing recording");
    for re_adopt in [false, true] {
        if re_adopt {
            attached = hito
                .catalog
                .adopt_provider(
                    &actor,
                    &youtube_installation.id,
                    AdoptProviderRecording {
                        installation_revision: youtube_installation.revision,
                        provider_recording_id: "youtube/candidate".into(),
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(attached.id, recording);
        assert_eq!(attached.title, "My title");
        assert_eq!(attached.artist, "My artist");
        assert_eq!(attached.inherited_title, "Spotify title");
        assert_eq!(attached.inherited_artist, "Spotify artist");
        assert_eq!(attached.library_id, inherited.library_id);
        assert_eq!(attached.release_id, inherited.release_id);
        assert_eq!(attached.album.as_deref(), Some("Spotify album"));
        assert_eq!(attached.duration_ms, 90_000);
        assert!(attached.available);
        assert!(attached.liked);
        assert_eq!(
            hito.catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap()
                .total,
            2
        );
        let after = hito
            .collection
            .playlist(&actor, &playlist_id)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(&after.origin).unwrap(), origin);
        assert_eq!(
            after.entries.iter().map(entry_identity).collect::<Vec<_>>(),
            identities
        );
        for index in [0, 2] {
            assert!(after.entries[index].available);
            assert!(after.entries[index].liked);
            assert_eq!(after.entries[index].title, "My title");
            assert_eq!(after.entries[index].artist, "My artist");
        }
        assert!(!after.entries[1].available);
        let metadata = hito
            .catalog
            .recording_metadata(&actor, &recording, Default::default())
            .await
            .unwrap();
        assert_eq!(metadata.title.operation_id, before.title.operation_id);
        assert_eq!(metadata.artist.operation_id, before.artist.operation_id);
        assert_eq!(
            serde_json::to_value(&metadata.selected_observation).unwrap(),
            selected
        );
        assert_eq!(metadata.observations.items.len(), 2);
        let observed = metadata
            .observations
            .items
            .iter()
            .find(|item| {
                item.plugin_installation_id.as_deref() == Some(youtube_installation.id.as_str())
            })
            .expect("the attached provider retains its own metadata observation");
        assert_eq!(observed.provider, "org.hito.youtube-fixture");
        assert_eq!(
            observed.provider_recording_id.as_deref(),
            Some("youtube/candidate")
        );
        assert_eq!(observed.title, "Different YouTube title");
        assert_eq!(observed.duration_ms, 91_000);
        let progress = hito
            .collection
            .activity(&actor)
            .await
            .unwrap()
            .history
            .remove(0);
        assert_eq!(
            (
                progress.id,
                progress.position_ms,
                progress.speed,
                progress.updated_at
            ),
            (
                history.id.clone(),
                history.position_ms,
                history.speed,
                history.updated_at
            )
        );
        assert_eq!(
            hito.collection
                .saved_position(&actor, recording_id)
                .await
                .unwrap()
                .unwrap()
                .position_ms,
            4_200
        );
        let sources = hito.catalog.sources(&actor, &recording).await.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].recording_id, recording);
        assert!(sources[0].ready);
        let grant = hito
            .playback
            .preferred_playback_source(&actor, &recording)
            .await
            .unwrap();
        assert!(!grant.is_local());
        assert_eq!(grant.source_id(), sources[0].id);
        let OpenedAudio::Content(mut content) = grant.open(Default::default()).await.unwrap()
        else {
            panic!("the attached source must provide the owned audio");
        };
        let mut body = content.body.take().unwrap();
        let mut received = Vec::new();
        while let Some(chunk) = body.chunk().await.unwrap() {
            received.extend_from_slice(&chunk);
        }
        assert_eq!(received, original);
    }
}

fn entry_identity(entry: &PlaylistEntry) -> Value {
    json!({
        "entry_id": entry.entry_id,
        "recording_id": entry.recording_id,
        "position": entry.position,
        "origin": entry.origin,
    })
}
