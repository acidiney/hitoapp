use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{Action, AdoptProviderRecording, ImportProviderPlaylist};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn importing_changed_metadata_preserves_adopted_sources_and_personal_collection_state() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-existing").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists", "sources"]).await;
    server
        .reply(
            "v1/recordings/detail",
            json!({
                "id": "recording/existing", "title": "Original title", "artist": "Original artist",
                "duration_ms": 90_000, "revision": "original"
            }),
        )
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items": [
                {"id": "audio/existing", "label": "Existing source", "ready": true}
            ]}),
        )
        .await;
    let adopted = hito
        .catalog
        .adopt_provider(
            &actor,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: installed.revision,
                provider_recording_id: "recording/existing".into(),
            },
        )
        .await
        .unwrap();
    assert!(adopted.available);
    let recording_id = Uuid::parse_str(&adopted.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4_200,
                speed: 1.25,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "My title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My artist".into(),
            },
        ],
    )
    .await;
    let before_sources =
        serde_json::to_value(hito.catalog.sources(&actor, &adopted.id).await.unwrap()).unwrap();
    assert_eq!(before_sources.as_array().unwrap().len(), 1);
    assert_eq!(before_sources[0]["ready"], true);
    let before_metadata = hito
        .catalog
        .recording_metadata(&actor, &adopted.id, Default::default())
        .await
        .unwrap();
    let before_history = hito
        .collection
        .activity(&actor)
        .await
        .unwrap()
        .history
        .remove(0);
    fixture::playlist(&server, "playlist/existing", "changed-metadata", vec![
        json!({"occurrence_id": "existing", "item": {"kind": "recording", "recording": {
            "id": "recording/existing", "title": "Changed provider title", "artist": "Changed provider artist",
            "duration_ms": 91_000, "revision": "changed"
        }}}),
        json!({"occurrence_id": "new", "item": {"kind": "recording", "recording": {
            "id": "recording/new", "title": "New metadata-only recording", "artist": "Fixture artist"
        }}}),
    ]).await;
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/existing-selection".into(),
            },
        )
        .await
        .unwrap();
    let detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(detail.entries.len(), 2);
    assert_eq!(
        detail.entries[0].recording_id.as_deref(),
        Some(adopted.id.as_str())
    );
    assert!(detail.entries[0].available);
    assert!(detail.entries[0].liked);
    assert_eq!(detail.entries[0].title, "My title");
    assert_eq!(detail.entries[0].artist, "My artist");
    assert_eq!(detail.entries[0].inherited_title, "Changed provider title");
    assert_eq!(
        detail.entries[0].inherited_artist,
        "Changed provider artist"
    );
    assert!(!detail.entries[1].available);
    let new_recording = detail.entries[1].recording_id.as_deref().unwrap();
    assert_ne!(new_recording, adopted.id);
    assert!(
        hito.catalog
            .sources(&actor, new_recording)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&actor, &adopted.id).await.unwrap()).unwrap(),
        before_sources
    );
    let after_metadata = hito
        .catalog
        .recording_metadata(&actor, &adopted.id, Default::default())
        .await
        .unwrap();
    assert_eq!(after_metadata.title.personal.as_deref(), Some("My title"));
    assert_eq!(after_metadata.artist.personal.as_deref(), Some("My artist"));
    assert_eq!(
        after_metadata.title.operation_id,
        before_metadata.title.operation_id
    );
    assert_eq!(
        after_metadata.artist.operation_id,
        before_metadata.artist.operation_id
    );
    assert_eq!(after_metadata.observations.items.len(), 2);
    let after_history = hito
        .collection
        .activity(&actor)
        .await
        .unwrap()
        .history
        .remove(0);
    assert_eq!(after_history.id, adopted.id);
    assert_eq!(after_history.position_ms, before_history.position_ms);
    assert_eq!(after_history.speed, before_history.speed);
    assert_eq!(after_history.updated_at, before_history.updated_at);
    assert_eq!(
        hito.collection
            .saved_position(&actor, recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        4_200
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        2
    );
    assert_eq!(server.requests("v1/recordings/detail").await.len(), 1);
    assert_eq!(server.requests("v1/sources").await.len(), 1);
}
