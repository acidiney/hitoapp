#[path = "fixtures/collection_artwork.rs"]
mod fixture;
mod support;

use hito_core::{Action, Actor, ImportProviderPlaylist, PluginInstallation, Services};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn existing_renamed_copies_retrieve_their_exact_provider_playlist_cover_without_changing_saved_state()
 {
    let provider = fixture::ArtworkServer::start().await;
    let (_directory, hito, actor, _library, _music) = support::library("playlist-artwork").await;
    let installed = provider.connect(&hito, &actor).await;
    let copies = [
        import_copy(&hito, &actor, &installed).await,
        import_copy(&hito, &actor, &installed).await,
    ];
    assert_ne!(copies[0], copies[1]);
    let catalog = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(catalog.total, 1);
    let recording = &catalog.items[0];
    let release = recording.release_id.as_ref().unwrap();
    let mut entry_ids = std::collections::HashSet::new();
    for copy in copies {
        let detail = hito
            .collection
            .playlist(&actor, &copy.to_string())
            .await
            .unwrap();
        assert_eq!(
            detail.origin.unwrap().provider_playlist_id,
            fixture::PLAYLIST_ID
        );
        assert_eq!(detail.entries.len(), 2);
        for entry in detail.entries {
            assert_eq!(entry.recording_id.as_deref(), Some(recording.id.as_str()));
            assert!(entry_ids.insert(entry.entry_id));
        }
    }
    assert_eq!(entry_ids.len(), 4);
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistRename {
                playlist_id: copies[0],
                name: "My private first copy".into(),
            },
            Action::PlaylistRename {
                playlist_id: copies[1],
                name: "My other private copy".into(),
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "My retained recording".into(),
            },
            Action::ReleaseTitleSet {
                release_id: Uuid::parse_str(release).unwrap(),
                title: "My retained album".into(),
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
    hito.catalog
        .enqueue_recording_artwork(&actor, &recording.id)
        .await
        .unwrap();
    assert!(
        hito.catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    hito.catalog
        .enqueue_release_artwork(&actor, release)
        .await
        .unwrap();
    assert!(
        hito.catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    let before = saved_state(&hito, &actor, copies, &recording.id).await;

    let mut receipts = Vec::new();
    for copy in copies {
        let receipt = hito
            .catalog
            .enqueue_playlist_artwork(&actor, &copy.to_string())
            .await
            .unwrap();
        let receipt = serde_json::to_value(receipt).unwrap();
        assert_eq!(receipt["playlist_id"], copy.to_string());
        assert_eq!(receipt["status"], "queued");
        assert!(receipt.get("recording_id").is_none());
        assert!(receipt.get("release_id").is_none());
        receipts.push(receipt);
    }
    assert_ne!(receipts[0]["id"], receipts[1]["id"]);
    assert_eq!(
        provider.requests().await,
        [
            ArtworkRequest {
                entity: ArtworkEntity::Recording,
                provider_id: fixture::RECORDING_ID.into()
            },
            ArtworkRequest {
                entity: ArtworkEntity::Release,
                provider_id: fixture::RELEASE_ID.into()
            },
        ]
    );
    for _ in copies {
        assert!(
            hito.catalog
                .run_next_artwork_job_until(std::future::pending::<()>())
                .await
                .unwrap()
        );
    }
    for (copy, receipt) in copies.into_iter().zip(receipts) {
        let artwork = hito
            .catalog
            .playlist_artwork(&actor, &copy.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(artwork.content_type, "image/png");
        assert_eq!(artwork.bytes, fixture::PLAYLIST_COVER);
        assert_ne!(artwork.bytes, fixture::RECORDING_COVER);
        assert_ne!(artwork.bytes, fixture::RELEASE_COVER);
        let retained = serde_json::to_value(
            hito.catalog
                .enqueue_playlist_artwork(&actor, &copy.to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(retained["id"], receipt["id"]);
        assert_eq!(retained["status"], "ready");
    }
    assert_eq!(
        hito.catalog
            .artwork(&actor, &recording.id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        fixture::RECORDING_COVER
    );
    assert_eq!(
        hito.catalog
            .release_artwork(&actor, release)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        fixture::RELEASE_COVER
    );
    assert_eq!(
        provider.requests().await,
        [
            ArtworkRequest {
                entity: ArtworkEntity::Recording,
                provider_id: fixture::RECORDING_ID.into()
            },
            ArtworkRequest {
                entity: ArtworkEntity::Release,
                provider_id: fixture::RELEASE_ID.into()
            },
            ArtworkRequest {
                entity: ArtworkEntity::Playlist,
                provider_id: fixture::PLAYLIST_ID.into()
            },
            ArtworkRequest {
                entity: ArtworkEntity::Playlist,
                provider_id: fixture::PLAYLIST_ID.into()
            },
        ]
    );
    assert_eq!(
        saved_state(&hito, &actor, copies, &recording.id).await,
        before
    );
}

async fn import_copy(hito: &Services, actor: &Actor, installed: &PluginInstallation) -> Uuid {
    hito.collection
        .import_provider_playlist(
            actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference:
                    "https://provider.invalid/submitted-link-is-not-the-playlist-id".into(),
            },
        )
        .await
        .unwrap()
        .playlist_id
}

async fn saved_state(hito: &Services, actor: &Actor, copies: [Uuid; 2], recording: &str) -> Value {
    json!({
        "recordings": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "albums": hito.catalog.releases(actor, Default::default()).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "first": hito.collection.playlist(actor, &copies[0].to_string()).await.unwrap(),
        "second": hito.collection.playlist(actor, &copies[1].to_string()).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "position_ms": hito.collection.saved_position(actor, Uuid::parse_str(recording).unwrap())
            .await.unwrap().unwrap().position_ms,
    })
}
