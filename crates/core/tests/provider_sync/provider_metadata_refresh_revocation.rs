#[path = "../fixtures/provider_metadata_refresh.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{Action, Error, RefreshImportedPlaylistMetadata};
use std::time::Duration;
use tokio::task::JoinSet;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
enum AuthorityChange {
    RemoveConnection,
    DeleteCopy,
}

#[tokio::test]
async fn authority_revocation_during_a_held_snapshot_cannot_publish_or_leave_a_receipt() {
    for change in [
        AuthorityChange::RemoveConnection,
        AuthorityChange::DeleteCopy,
    ] {
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
            .expect("The provider snapshot must be held before changing authority");
        tokio::time::timeout(Duration::from_secs(5), async {
            match change {
                AuthorityChange::RemoveConnection => f
                    .hito
                    .plugins
                    .remove(&f.actor, &f.installation.id, revision)
                    .await
                    .unwrap(),
                AuthorityChange::DeleteCopy => {
                    support::synchronize(
                        &f.hito,
                        &f.actor,
                        vec![Action::PlaylistDelete {
                            playlist_id: f.first.playlist_id,
                        }],
                    )
                    .await
                }
            }
        })
        .await
        .expect("The public authority change must commit while the provider is held");
        let after_change = fixture::private_state(&f, &recording).await;
        for field in [
            "catalog",
            "albums",
            "artists",
            "metadata",
            "sources",
            "source_choice",
            "activity",
            "position",
            "second",
        ] {
            assert!(
                after_change[field] == before[field],
                "Authority changes must retain unrelated recording, source, activity and copy state"
            );
        }
        let held_requests = fixture::request_counts(&f).await;
        gate.release();
        let result = tokio::time::timeout(Duration::from_secs(5), pending.join_next())
            .await
            .expect("The rejected workflow must finish after the held response is released")
            .expect("The owned metadata request must still be joined")
            .expect("The owned metadata task must not panic");
        assert!(
            matches!(
                (change, result),
                (AuthorityChange::RemoveConnection, Err(Error::Conflict(_)))
                    | (AuthorityChange::DeleteCopy, Err(Error::NotFound))
            ),
            "The stale snapshot must fail with the authority error"
        );
        assert!(
            fixture::private_state(&f, &recording).await == after_change,
            "A rejected snapshot must not publish metadata, events, entries, activity or downloads"
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
                            installation_revision: revision
                        }
                    )
                    .await,
                Err(Error::NotFound)
            ),
            "A rejected snapshot must not leave a replayable receipt"
        );
        assert_eq!(
            fixture::request_counts(&f).await,
            held_requests,
            "Revocation must prevent further provider requests and all audio requests"
        );
    }
}
