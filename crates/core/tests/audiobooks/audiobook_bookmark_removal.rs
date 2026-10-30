#[path = "../fixtures/audiobooks_library.rs"]
mod library;
#[path = "../fixtures/audiobooks_bookmarks.rs"]
mod markers;
#[path = "../fixtures/audiobooks_peer.rs"]
mod peer;
use hito_core::test_support::support;

use hito_core::{Action, Error};
use markers::{
    Marker, accept, assert_event_added, assert_visible_unchanged, batch, removal, snapshot,
};
use serde_json::Value;
use uuid::Uuid;

#[tokio::test]
async fn bookmark_removal_wins_over_replay_and_late_create_without_crossing_private_books() {
    let library = library::Library::prepare().await;
    let peer = peer::Peer::prepare(&library).await;
    let musical = library.snapshot().await;
    let alternate =
        Uuid::parse_str(musical["unrelated"]["playlist"]["id"].as_str().unwrap()).unwrap();
    let alternate_entry = Uuid::parse_str(
        musical["unrelated"]["entries"][0]["entry_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(
        peer.entry != library.entries[0],
        "The second account must own a separate chapter occurrence"
    );
    library::apply(
        &library.hito,
        &library.actor,
        vec![
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            Action::AudiobookCreate {
                playlist_id: alternate,
            },
        ],
    )
    .await;
    let first = Marker {
        id: Uuid::new_v4(),
        audiobook_id: library.playlist,
        entry_id: library.entries[0],
        recording_id: library.recordings[0],
        position_ms: 250,
        label: "Private first return point",
    };
    let second = Marker {
        id: Uuid::new_v4(),
        audiobook_id: library.playlist,
        entry_id: library.entries[1],
        recording_id: library.recordings[1],
        position_ms: 550,
        label: "Private retained return point",
    };
    let first_create = Uuid::new_v4();
    accept(&library, first.create(first_create)).await;
    accept(&library, second.create(Uuid::new_v4())).await;
    let initial = snapshot(&library, alternate).await;
    let other_before = peer.snapshot(&library).await;
    let initial_markers = initial["book"]["bookmarks"].as_array().unwrap();
    assert!(
        initial_markers.len() == 2
            && initial_markers[0]["id"] == first.id.to_string()
            && initial_markers[1]["id"] == second.id.to_string(),
        "The removal workflow must begin with two independently anchored private bookmarks"
    );
    assert!(
        initial["musical"] == musical,
        "Initial grouping and bookmarks must preserve the original private musical snapshot"
    );

    let denied = library
        .hito
        .collection
        .apply(
            &peer.actor,
            batch(removal(library.playlist, first.id, Uuid::new_v4())),
        )
        .await;
    assert!(
        matches!(denied, Err(Error::NotFound)),
        "Another shared-library account must not remove a private bookmark by known book and marker identities"
    );
    let denied = library
        .hito
        .collection
        .apply(
            &library.actor,
            batch(removal(alternate, first.id, Uuid::new_v4())),
        )
        .await;
    assert!(
        matches!(denied, Err(Error::NotFound)),
        "A different owned book must not authorize removal of an existing marker from its original book"
    );
    assert!(
        snapshot(&library, alternate).await == initial
            && peer.snapshot(&library).await == other_before,
        "Denied removals must preserve both accounts, both owned books and every accepted event"
    );

    let remove_first = Uuid::new_v4();
    accept(&library, removal(library.playlist, first.id, remove_first)).await;
    let removed = snapshot(&library, alternate).await;
    let remaining: Vec<Value> = initial_markers
        .iter()
        .filter(|marker| marker["id"] != first.id.to_string())
        .cloned()
        .collect();
    assert!(
        remaining.len() == 1 && remaining[0]["id"] == second.id.to_string(),
        "The independently created second bookmark must remain after removing the first"
    );
    let mut expected = initial.clone();
    expected["book"]["bookmarks"] = Value::Array(remaining);
    expected["events"] = removed["events"].clone();
    assert!(
        removed == expected,
        "Removing one marker must change only that visible bookmark and its accepted event"
    );
    assert_event_added(&initial, &removed, remove_first);

    accept(&library, removal(library.playlist, first.id, remove_first)).await;
    accept(&library, first.create(first_create)).await;
    assert!(
        snapshot(&library, alternate).await == removed,
        "Exact removal replay and stale creation replay must not duplicate events or resurrect the marker"
    );
    let fresh_create = Uuid::new_v4();
    accept(&library, first.create(fresh_create)).await;
    let fresh_noop = snapshot(&library, alternate).await;
    assert_visible_unchanged(&removed, &fresh_noop);
    assert_event_added(&removed, &fresh_noop, fresh_create);

    let late = Marker {
        id: Uuid::new_v4(),
        position_ms: 275,
        label: "Private later return point",
        ..first
    };
    let remove_unknown = Uuid::new_v4();
    accept(&library, removal(library.playlist, late.id, remove_unknown)).await;
    let unknown_removed = snapshot(&library, alternate).await;
    assert_visible_unchanged(&fresh_noop, &unknown_removed);
    assert_event_added(&fresh_noop, &unknown_removed, remove_unknown);
    let late_create = Uuid::new_v4();
    accept(&library, late.create(late_create)).await;
    let late_noop = snapshot(&library, alternate).await;
    assert_visible_unchanged(&unknown_removed, &late_noop);
    assert_event_added(&unknown_removed, &late_noop, late_create);
    let transplanted = Marker {
        audiobook_id: alternate,
        entry_id: alternate_entry,
        ..late
    };
    let denied = library
        .hito
        .collection
        .apply(&library.actor, batch(transplanted.create(Uuid::new_v4())))
        .await;
    assert!(
        matches!(denied, Err(Error::Conflict(_))),
        "A remove-before-create tombstone must retain its original book identity rather than moving to another owned book"
    );
    assert!(
        snapshot(&library, alternate).await == late_noop
            && peer.snapshot(&library).await == other_before,
        "A rejected tombstone transplant must leave all private state and event history unchanged"
    );
    library.assert_no_downloads().await;
    peer.assert_no_downloads(&library).await;
    library.assert_originals_unchanged();
    let reopened = library.reopen().await;
    assert!(
        snapshot(&reopened, alternate).await == late_noop
            && peer.snapshot(&reopened).await == other_before,
        "Known and remove-before-create tombstones must survive reopening without changing another marker or account"
    );
    reopened.assert_no_downloads().await;
    peer.assert_no_downloads(&reopened).await;
    reopened.assert_originals_unchanged();
}
