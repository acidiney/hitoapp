use hito_core::test_support::collection_artwork as fixture;
use hito_core::test_support::support;

use hito_core::{Action, Actor, ImportProviderPlaylist, PluginInstallation, Services};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn existing_copies_retain_the_exact_album_cover_without_replacing_recording_artwork() {
    let provider = fixture::ArtworkServer::start().await;
    let (_directory, hito, actor, _library, _music) = support::library("collection-artwork").await;
    let installed = provider.connect(&hito, &actor).await;
    let first = import_copy(&hito, &actor, &installed).await;
    let second = import_copy(&hito, &actor, &installed).await;
    assert_ne!(first, second);
    let recordings = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    let recording = &recordings.items[0];
    let release = recording.release_id.as_ref().unwrap();
    let albums = hito
        .catalog
        .releases(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    assert_eq!(&albums.items[0].id, release);
    assert!(albums.items[0].artist.is_none());
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                title: "My retained recording".into(),
            },
            Action::ReleaseTitleSet {
                release_id: Uuid::parse_str(release).unwrap(),
                title: "My retained album".into(),
            },
            Action::LikeSet {
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                liked: true,
            },
            Action::ProgressSet {
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                position_ms: 4_200,
                speed: 1.25,
            },
            Action::PlaylistRename {
                playlist_id: first,
                name: "My retained copy".into(),
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
    assert_eq!(
        hito.catalog
            .artwork(&actor, &recording.id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        fixture::RECORDING_COVER
    );
    assert!(
        hito.catalog
            .release_artwork(&actor, release)
            .await
            .unwrap()
            .is_none()
    );
    let before = saved_state(&hito, &actor, [first, second], &recording.id).await;

    let queued = hito
        .catalog
        .enqueue_release_artwork(&actor, release)
        .await
        .unwrap();
    let queued = serde_json::to_value(queued).unwrap();
    assert_eq!(queued["release_id"], release.as_str());
    assert_eq!(queued["status"], "queued");
    assert_eq!(
        provider.requests().await,
        [ArtworkRequest {
            entity: ArtworkEntity::Recording,
            provider_id: fixture::RECORDING_ID.into(),
        }]
    );
    assert!(
        hito.catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    let artwork = hito
        .catalog
        .release_artwork(&actor, release)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(artwork.content_type, "image/png");
    assert_eq!(artwork.bytes, fixture::RELEASE_COVER);
    assert_ne!(artwork.bytes, fixture::RECORDING_COVER);
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
    assert_eq!(
        saved_state(&hito, &actor, [first, second], &recording.id).await,
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
                playlist_reference: "https://provider.invalid/owned-artwork".into(),
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
