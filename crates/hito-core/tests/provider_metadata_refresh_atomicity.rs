#[path = "fixtures/provider_metadata_refresh.rs"]
mod fixture;
#[path = "fixtures/provider_metadata_refresh_snapshot.rs"]
mod snapshot;
mod support;

use hito_core::{Error, RefreshImportedPlaylistMetadata};
use uuid::Uuid;

#[tokio::test]
async fn conflicting_target_release_metadata_rolls_back_every_write_and_leaves_the_request_retryable()
 {
    let f = fixture::Library::start().await;
    let recording = fixture::seed_private_state(&f).await;
    snapshot::publish(&f, true).await;
    let before = fixture::private_state(&f, &recording).await;
    let playlist = f.first.playlist_id.to_string();
    let request_id = Uuid::new_v4();
    let request = || RefreshImportedPlaylistMetadata {
        request_id,
        installation_revision: f.installation.revision,
    };
    let result = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &playlist, request())
        .await;
    assert!(
        matches!(result, Err(Error::Upstream(_))),
        "Two targeted recordings must not assign conflicting metadata to one provider release"
    );
    assert!(
        fixture::private_state(&f, &recording).await == before,
        "The entire failed publication must roll back catalog rows, observations, private state and events"
    );
    let failed_requests = fixture::request_counts(&f).await;
    snapshot::publish(&f, false).await;
    let completed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &playlist, request())
        .await
        .expect(
            "The same unaccepted request must remain retryable with a consistent provider snapshot",
        );
    assert_eq!(completed.updated_recordings, 2);
    assert_eq!(completed.unchanged_recordings, 0);
    assert_eq!(completed.unavailable_recordings, 0);
    let after = fixture::private_state(&f, &recording).await;
    let albums = f
        .hito
        .catalog
        .releases(&f.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    assert!(albums.items[0].artist.is_none() && albums.items[0].artist_id.is_none());
    for old in before["all_metadata"].as_array().unwrap() {
        let current = after["all_metadata"]
            .as_array()
            .unwrap()
            .iter()
            .find(|current| current["recording_id"] == old["recording_id"])
            .unwrap();
        assert_eq!(
            current["observations"]["items"].as_array().unwrap().len(),
            old["observations"]["items"].as_array().unwrap().len() + 1
        );
        assert!(current["selected_observation"]["release_id"].is_string());
        for field in ["title", "artist"] {
            assert!(current[field]["personal"] == old[field]["personal"]);
            assert!(current[field]["operation_id"] == old[field]["operation_id"]);
        }
    }
    let published = f
        .hito
        .collection
        .events(&f.actor, before["events"]["cursor"].as_i64().unwrap())
        .await
        .unwrap();
    assert_eq!(published.operations.len(), 1);
    let event = serde_json::to_value(&published.operations[0]).unwrap();
    assert_eq!(event["publication"]["kind"], "playlist_metadata_refreshed");
    assert_eq!(
        event["publication"]["recordings"].as_array().unwrap().len(),
        2
    );
    assert!(event["publication"].get("entries").is_none());
    let accepted_requests = fixture::request_counts(&f).await;
    for ((path, old_origin, old_alternate), (after_path, new_origin, new_alternate)) in
        failed_requests.iter().zip(&accepted_requests)
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
            "A failed publication must not leave a receipt that bypasses its new snapshot"
        );
        assert_eq!(new_alternate, old_alternate);
    }
    f.origin.offline().await;
    f.alternate.offline().await;
    let replayed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &playlist, request())
        .await
        .unwrap();
    assert!(serde_json::to_vec(&replayed).unwrap() == serde_json::to_vec(&completed).unwrap());
    assert_eq!(fixture::request_counts(&f).await, accepted_requests);
    assert!(fixture::private_state(&f, &recording).await == after);
}
