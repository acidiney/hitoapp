#[path = "fixtures/provider_metadata_refresh.rs"]
mod fixture;
#[path = "fixtures/provider_metadata_refresh_snapshot.rs"]
mod snapshot;
mod support;

use hito_core::{Action, Error, RefreshImportedPlaylistMetadata};
use std::time::Duration;
use tokio::task::JoinSet;
use uuid::Uuid;

#[tokio::test]
async fn removing_every_imported_occurrence_rejects_the_held_scope_before_any_metadata_is_written()
{
    let f = fixture::Library::start().await;
    let recording = fixture::seed_private_state(&f).await;
    snapshot::publish(&f, false).await;
    let playlist = f.first.playlist_id.to_string();
    let original = f
        .hito
        .collection
        .playlist(&f.actor, &playlist)
        .await
        .unwrap();
    let remaining = original.entries[1].recording_id.as_deref().unwrap();
    let removals: Vec<_> = original
        .entries
        .iter()
        .filter(|entry| entry.recording_id.as_deref() == Some(recording.as_str()))
        .map(|entry| Action::EntryRemove {
            playlist_id: f.first.playlist_id,
            entry_id: Uuid::parse_str(&entry.entry_id).unwrap(),
        })
        .collect();
    assert_eq!(
        removals.len(),
        2,
        "Both imported occurrences must be removed to change recording membership"
    );
    let before = fixture::private_state(&f, &recording).await;
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
        .expect("The provider must hold the original recording scope before membership changes");
    tokio::time::timeout(
        Duration::from_secs(5),
        support::synchronize(&f.hito, &f.actor, removals),
    )
    .await
    .expect("Public entry removals must commit while the snapshot remains held");
    let after_change = fixture::private_state(&f, &recording).await;
    for field in [
        "catalog",
        "albums",
        "artists",
        "metadata",
        "all_metadata",
        "sources",
        "source_choice",
        "activity",
        "position",
        "second",
    ] {
        assert!(
            after_change[field] == before[field],
            "Entry removal must preserve unrelated recording and private state"
        );
    }
    let changed = f
        .hito
        .collection
        .events(&f.actor, before["events"]["cursor"].as_i64().unwrap())
        .await
        .unwrap();
    assert_eq!(changed.operations.len(), 2);
    gate.release();
    let result = tokio::time::timeout(Duration::from_secs(5), pending.join_next())
        .await
        .expect("The held request must finish after membership changes")
        .expect("The owned request must remain joined")
        .expect("The owned request must not panic");
    assert!(
        matches!(result, Err(Error::Conflict(_))),
        "The old target witness must reject the changed recording set"
    );
    assert!(
        fixture::private_state(&f, &recording).await == after_change,
        "A stale witness must not write partial observations, albums or publications"
    );

    gate.open();
    let fresh = tokio::time::timeout(
        Duration::from_secs(5),
        f.hito.collection.refresh_imported_playlist_metadata(
            &f.actor,
            &playlist,
            RefreshImportedPlaylistMetadata {
                request_id: Uuid::new_v4(),
                installation_revision: revision,
            },
        ),
    )
    .await
    .expect("A new request must acquire the released capacity")
    .expect("A new request must use the current imported recording set");
    assert_eq!(fresh.updated_recordings, 1);
    assert_eq!(fresh.unchanged_recordings, 0);
    assert_eq!(fresh.unavailable_recordings, 0);
    let after = fixture::private_state(&f, &recording).await;
    assert!(
        after["metadata"] == before["metadata"],
        "The removed recording must not be enriched by the new request"
    );
    let updated = f
        .hito
        .catalog
        .recording_metadata(&f.actor, remaining, Default::default())
        .await
        .unwrap();
    assert!(
        updated
            .selected_observation
            .as_ref()
            .unwrap()
            .release_id
            .is_some()
    );
    let events = f
        .hito
        .collection
        .events(&f.actor, after_change["events"]["cursor"].as_i64().unwrap())
        .await
        .unwrap();
    assert_eq!(events.operations.len(), 1);
    let event = serde_json::to_value(&events.operations[0]).unwrap();
    let recordings = event["publication"]["recordings"].as_array().unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0]["recording_id"], remaining);
    assert!(event["publication"].get("entries").is_none());
    let _requests = fixture::request_counts(&f).await;
}
