#[path = "fixtures/provider_metadata_refresh.rs"]
mod fixture;
mod support;

use hito_core::{Error, RefreshImportedPlaylistMetadata};
use std::time::Duration;
use tokio::task::JoinSet;
use uuid::Uuid;

#[tokio::test]
async fn a_held_owner_rejects_same_account_work_until_its_receipt_can_be_replayed() {
    let f = fixture::Library::start().await;
    let recording = fixture::seed_private_state(&f).await;
    f.publish_verified_album().await;
    let before = fixture::private_state(&f, &recording).await;
    let playlist = f.first.playlist_id.to_string();
    let request_id = Uuid::new_v4();
    let revision = f.installation.revision;
    let gate = f.origin.hold("v1/playlists/items").await;
    let mut pending = JoinSet::new();
    let (collection, actor, requested_playlist) =
        (f.hito.collection.clone(), f.actor.clone(), playlist.clone());
    pending.spawn(async move {
        collection
            .refresh_imported_playlist_metadata(
                &actor,
                &requested_playlist,
                RefreshImportedPlaylistMetadata {
                    request_id,
                    installation_revision: revision,
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), gate.entered())
        .await
        .expect("The first snapshot must be held before testing admission");
    let held_requests = fixture::request_counts(&f).await;
    for attempted in [Uuid::new_v4(), request_id] {
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            f.hito.collection.refresh_imported_playlist_metadata(
                &f.actor,
                &playlist,
                RefreshImportedPlaylistMetadata {
                    request_id: attempted,
                    installation_revision: revision,
                },
            ),
        )
        .await
        .expect(
            "Unaccepted same-account work must fail promptly instead of waiting for the provider",
        );
        assert!(
            matches!(result, Err(Error::Conflict(_))),
            "Both a new UUID and the held UUID must be busy before acceptance"
        );
    }
    assert_eq!(fixture::request_counts(&f).await, held_requests);
    assert!(
        fixture::private_state(&f, &recording).await == before,
        "Busy requests must not change metadata, entries, events or private activity"
    );
    gate.release();
    let completed = tokio::time::timeout(Duration::from_secs(5), pending.join_next())
        .await
        .expect("The released owner must finish")
        .expect("The owner must remain joined")
        .expect("The owner must not panic")
        .expect("The owner must publish its accepted metadata");
    assert_eq!(completed.updated_recordings, 1);
    let receipt = serde_json::to_vec(&completed).unwrap();
    let accepted = fixture::private_state(&f, &recording).await;
    let accepted_requests = fixture::request_counts(&f).await;
    f.origin.offline().await;
    f.alternate.offline().await;
    let replayed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(
            &f.actor,
            &playlist,
            RefreshImportedPlaylistMetadata {
                request_id,
                installation_revision: revision,
            },
        )
        .await
        .unwrap();
    assert!(serde_json::to_vec(&replayed).unwrap() == receipt);
    assert_eq!(fixture::request_counts(&f).await, accepted_requests);
    assert!(
        fixture::private_state(&f, &recording).await == accepted,
        "The formerly busy UUID must replay without another publication"
    );
}

#[tokio::test]
async fn aborting_a_future_while_its_snapshot_is_held_releases_admission_without_publication() {
    let f = fixture::Library::start().await;
    let recording = fixture::seed_private_state(&f).await;
    f.publish_verified_album().await;
    let before = fixture::private_state(&f, &recording).await;
    let cursor = before["events"]["cursor"].as_i64().unwrap();
    let playlist = f.first.playlist_id.to_string();
    let revision = f.installation.revision;
    let gate = f.origin.hold("v1/playlists/items").await;
    let mut pending = JoinSet::new();
    let (collection, actor, requested_playlist) =
        (f.hito.collection.clone(), f.actor.clone(), playlist.clone());
    pending.spawn(async move {
        collection
            .refresh_imported_playlist_metadata(
                &actor,
                &requested_playlist,
                RefreshImportedPlaylistMetadata {
                    request_id: Uuid::new_v4(),
                    installation_revision: revision,
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), gate.entered())
        .await
        .expect("The owner must be held before publication when it is aborted");
    let held_requests = fixture::request_counts(&f).await;
    pending.abort_all();
    let cancelled = tokio::time::timeout(Duration::from_secs(5), pending.join_next())
        .await
        .expect("The aborted owner future must finish dropping")
        .expect("The owner must remain joined");
    assert!(cancelled.is_err_and(|error| error.is_cancelled()));
    assert!(pending.is_empty());
    assert!(
        fixture::private_state(&f, &recording).await == before,
        "Aborting at the held snapshot boundary must leave all state unchanged"
    );
    gate.open();
    let fresh_id = Uuid::new_v4();
    let fresh = tokio::time::timeout(
        Duration::from_secs(5),
        f.hito.collection.refresh_imported_playlist_metadata(
            &f.actor,
            &playlist,
            RefreshImportedPlaylistMetadata {
                request_id: fresh_id,
                installation_revision: revision,
            },
        ),
    )
    .await
    .expect("A fresh request must acquire the released account capacity")
    .expect("The fresh request must complete without a latent owner publication");
    assert_eq!(fresh.updated_recordings, 1);
    let published = f.hito.collection.events(&f.actor, cursor).await.unwrap();
    assert_eq!(published.operations.len(), 1);
    assert!(!published.has_more);
    let after_requests = fixture::request_counts(&f).await;
    for ((path, old_origin, old_alternate), (after_path, new_origin, new_alternate)) in
        held_requests.iter().zip(&after_requests)
    {
        assert_eq!(path, after_path);
        let expected = match *path {
            "v1/playlists/detail" => 2,
            "v1/playlists/items" => 1,
            _ => 0,
        };
        assert_eq!(
            *new_origin,
            old_origin + expected,
            "Only the fresh request may continue the provider workflow"
        );
        assert_eq!(new_alternate, old_alternate);
    }
    let accepted = fixture::private_state(&f, &recording).await;
    f.origin.offline().await;
    f.alternate.offline().await;
    let replay = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(
            &f.actor,
            &playlist,
            RefreshImportedPlaylistMetadata {
                request_id: fresh_id,
                installation_revision: revision,
            },
        )
        .await
        .unwrap();
    assert!(serde_json::to_vec(&replay).unwrap() == serde_json::to_vec(&fresh).unwrap());
    assert_eq!(fixture::request_counts(&f).await, after_requests);
    assert!(fixture::private_state(&f, &recording).await == accepted);
}
