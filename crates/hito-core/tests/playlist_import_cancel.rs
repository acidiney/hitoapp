#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{Actor, CollectionPublication, Error, ImportProviderPlaylist, Services, SyncEvent};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn cancelling_a_held_import_frees_control_admission_without_publishing_late() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-cancel").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/cancel", "snapshot-one", vec![
        json!({"occurrence_id": "retained", "item": {"kind": "recording", "recording": {
            "id": "recording/retained", "title": "Retained recording", "artist": "Fixture artist"
        }}}),
    ]).await;
    let before = public_state(&hito, &actor).await;
    assert_eq!(before["catalog"]["total"], 0);
    assert!(before["playlists"].as_array().unwrap().is_empty());
    assert!(before["feed"]["operations"].as_array().unwrap().is_empty());
    let gate = server.hold("v1/playlists/items").await;
    let request_id = Uuid::new_v4();
    let request = || ImportProviderPlaylist {
        request_id,
        installation_revision: installed.revision,
        playlist_reference: "https://provider.invalid/cancel-selection".into(),
    };
    let pending = {
        let (collection, actor, installation_id, request) = (
            hito.collection.clone(),
            actor.clone(),
            installed.id.clone(),
            request(),
        );
        tokio::spawn(async move {
            collection
                .import_provider_playlist(&actor, &installation_id, request)
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("the original import must be held inside its provider item response");
    assert_eq!(public_state(&hito, &actor).await, before);
    let mut competing = request();
    competing.request_id = Uuid::new_v4();
    let busy = tokio::time::timeout(
        Duration::from_secs(1),
        hito.collection
            .import_provider_playlist(&actor, &installed.id, competing),
    )
    .await
    .expect("occupied account control admission must fail promptly");
    assert!(matches!(busy, Err(Error::Conflict(_))));
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 1);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 1);

    pending.abort();
    let joined = tokio::time::timeout(Duration::from_secs(1), pending)
        .await
        .expect("cancelled import observers must join while the provider remains held");
    assert!(matches!(joined, Err(error) if error.is_cancelled()));
    assert_eq!(public_state(&hito, &actor).await, before);
    let retry = {
        let (collection, actor, installation_id, request) = (
            hito.collection.clone(),
            actor.clone(),
            installed.id.clone(),
            request(),
        );
        tokio::spawn(async move {
            collection
                .import_provider_playlist(&actor, &installation_id, request)
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect(
            "cancellation must free control admission before either provider handler is released",
        );
    assert_eq!(public_state(&hito, &actor).await, before);
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 2);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 2);
    gate.release();
    gate.release();
    let accepted = tokio::time::timeout(Duration::from_secs(3), retry)
        .await
        .expect("the exact retry must complete once its provider response is released")
        .unwrap()
        .unwrap();
    assert_eq!(accepted.request_id, request_id);
    let playlists = hito.collection.playlists(&actor).await.unwrap();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].id, accepted.playlist_id.to_string());
    let detail = hito
        .collection
        .playlist(&actor, &accepted.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(detail.entries.len(), 1);
    assert_eq!(detail.entries[0].title, "Retained recording");
    assert!(!detail.entries[0].available);
    let recording = detail.entries[0].recording_id.as_deref().unwrap();
    assert!(
        hito.catalog
            .sources(&actor, recording)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    let feed = hito.collection.events(&actor, 0).await.unwrap();
    assert_eq!(feed.operations.len(), 1);
    assert!(!feed.has_more);
    match &feed.operations[0] {
        SyncEvent::Publication {
            publication: CollectionPublication::PlaylistImported { snapshot, .. },
            ..
        } => {
            assert_eq!(snapshot.playlist_id, accepted.playlist_id);
            assert_eq!(snapshot.entries.len(), 1);
            assert_eq!(snapshot.recordings.len(), 1);
        }
        SyncEvent::Publication {
            publication:
                CollectionPublication::PlaylistFollowed { .. }
                | CollectionPublication::PlaylistFollowRefreshed { .. },
            ..
        } => panic!("the accepted import must not produce a follow publication"),
        SyncEvent::Publication {
            publication: CollectionPublication::PlaylistMetadataRefreshed { .. },
            ..
        } => panic!("the accepted import must not produce a metadata refresh publication"),
        SyncEvent::Operation { .. } => {
            panic!("the accepted import must produce one immutable publication")
        }
    }
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 3);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 2);
    let after = public_state(&hito, &actor).await;
    let replay = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    assert_eq!(replay.playlist_id, accepted.playlist_id);
    assert_eq!(public_state(&hito, &actor).await, after);
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 3);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 2);
}

async fn public_state(hito: &Services, actor: &Actor) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "feed": hito.collection.events(actor, 0).await.unwrap()
    })
}
