#[path = "fixtures/provider_metadata_refresh.rs"]
mod fixture;
mod support;

use hito_core::{Error, RefreshImportedPlaylistMetadata};
use uuid::Uuid;

#[tokio::test]
async fn an_albumless_snapshot_retains_a_noop_receipt_without_observations_or_publications() {
    let f = fixture::Library::start().await;
    let recording = fixture::seed_private_state(&f).await;
    let playlist = f.first.playlist_id.to_string();
    let before = fixture::private_state(&f, &recording).await;
    let requests_before = fixture::request_counts(&f).await;
    let request_id = Uuid::new_v4();
    let request = || RefreshImportedPlaylistMetadata {
        request_id,
        installation_revision: f.installation.revision,
    };
    let completed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &playlist, request())
        .await
        .unwrap();
    assert_eq!(completed.updated_recordings, 0);
    assert_eq!(completed.unchanged_recordings, 2);
    assert_eq!(completed.unavailable_recordings, 0);
    assert!(
        fixture::private_state(&f, &recording).await == before,
        "A no-op must retain observation IDs, events, private edits, sources, entries and activity"
    );
    let requests_after = fixture::request_counts(&f).await;
    for ((path, before_origin, before_alternate), (after_path, after_origin, after_alternate)) in
        requests_before.iter().zip(&requests_after)
    {
        assert_eq!(path, after_path);
        let expected = match *path {
            "v1/playlists/detail" => 2,
            "v1/playlists/items" => 1,
            _ => 0,
        };
        assert_eq!(*after_origin, before_origin + expected);
        assert_eq!(after_alternate, before_alternate);
    }
    let receipt = serde_json::to_vec(&completed).unwrap();
    f.origin.offline().await;
    f.alternate.offline().await;
    let replayed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &playlist, request())
        .await
        .unwrap();
    assert!(
        serde_json::to_vec(&replayed).unwrap() == receipt,
        "A no-op must return its exact retained receipt while providers are offline"
    );
    assert!(
        matches!(
            f.hito
                .collection
                .refresh_imported_playlist_metadata(
                    &f.actor,
                    &playlist,
                    RefreshImportedPlaylistMetadata {
                        request_id,
                        installation_revision: f.installation.revision + 1
                    }
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "A no-op receipt must reject conflicting request contents"
    );
    assert_eq!(fixture::request_counts(&f).await, requests_after);
    assert!(
        fixture::private_state(&f, &recording).await == before,
        "No-op replay and conflict rejection must preserve all retained state"
    );
}
