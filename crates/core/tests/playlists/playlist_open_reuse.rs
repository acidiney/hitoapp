use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{Action, ImportProviderPlaylist};
use serde_json::{Value, json};
use uuid::Uuid;

fn item(id: &str) -> Value {
    json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
        "id": format!("recording/{id}"), "title": format!("Tune {id}"), "artist": "Performer",
        "duration_ms": 60_000
    }}})
}

fn request(installation_revision: i64) -> ImportProviderPlaylist {
    ImportProviderPlaylist {
        request_id: Uuid::new_v4(),
        installation_revision,
        playlist_reference: "shared-reference".into(),
    }
}

#[tokio::test]
async fn opening_a_provider_playlist_twice_keeps_one_copy_and_plain_imports_still_copy() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-open-reuse").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/one", "snapshot-one", vec![item("a")]).await;

    let first = hito
        .collection
        .open_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    assert!(!first.reused);
    let second = hito
        .collection
        .open_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    assert!(second.reused);
    assert_eq!(first.playlist_id, second.playlist_id);
    assert_eq!(hito.collection.playlists(&actor).await.unwrap().len(), 1);

    hito.collection
        .import_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    assert_eq!(hito.collection.playlists(&actor).await.unwrap().len(), 2);
    let third = hito
        .collection
        .open_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    assert!(third.reused);
    assert_eq!(hito.collection.playlists(&actor).await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_changed_snapshot_reuses_the_copy_without_touching_personal_edits() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-open-changed").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/one", "snapshot-one", vec![item("a")]).await;
    let first = hito
        .collection
        .open_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistRename {
            playlist_id: first.playlist_id,
            name: "My own name".into(),
        }],
    )
    .await;
    fixture::playlist(
        &server,
        "playlist/one",
        "snapshot-two",
        vec![item("a"), item("b")],
    )
    .await;
    let again = hito
        .collection
        .open_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    assert!(again.reused);
    assert_eq!(again.playlist_id, first.playlist_id);
    let detail = hito
        .collection
        .playlist(&actor, &first.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(detail.playlist.name, "My own name");
    assert_eq!(detail.entries.len(), 1);
}

#[tokio::test]
async fn a_deleted_copy_is_not_reused() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-open-deleted").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(&server, "playlist/one", "snapshot-one", vec![item("a")]).await;
    let first = hito
        .collection
        .open_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistDelete {
            playlist_id: first.playlist_id,
        }],
    )
    .await;
    let again = hito
        .collection
        .open_provider_playlist(&actor, &installed.id, request(installed.revision))
        .await
        .unwrap();
    assert!(!again.reused);
    assert_ne!(again.playlist_id, first.playlist_id);
}
