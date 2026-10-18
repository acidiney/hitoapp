#[path = "fixtures/audiobooks_library.rs"]
mod library;
mod support;

use hito_core::{Action, Error, Operation, SyncBatch};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn raw_bookmark_label_limits_reject_padding_before_insert_and_existing_bookmark_acceptance() {
    let library = library::Library::prepare().await;
    library::apply(
        &library.hito,
        &library.actor,
        vec![Action::AudiobookCreate {
            playlist_id: library.playlist,
        }],
    )
    .await;
    let before = library.snapshot().await;
    let empty_book = book(&library).await;
    let initial_events = events(&library).await;
    assert!(
        empty_book["bookmarks"].as_array().unwrap().is_empty(),
        "The label regression must begin with an empty audiobook bookmark collection"
    );
    let padded = format!("{}x", " ".repeat(250_000));
    assert!(
        padded.len() > 256 && padded.trim() == "x" && padded.trim().len() == 1,
        "The regression label must exceed the raw field bound while its trimmed form remains short"
    );
    let bookmark_id = Uuid::new_v4();
    let first_operation = Uuid::new_v4();
    let rejected = library
        .hito
        .collection
        .apply(
            &library.actor,
            http_sized_batch(&library, bookmark_id, first_operation, &padded),
        )
        .await;
    assert!(
        matches!(rejected, Err(Error::Invalid(_))),
        "An oversized raw bookmark label must be rejected before creating a bookmark or storing a sync payload"
    );
    assert!(
        book(&library).await == empty_book && events(&library).await == initial_events,
        "A rejected raw label must leave no bookmark or accepted sync event"
    );
    assert!(
        library.snapshot().await == before,
        "Raw label rejection must preserve all existing musical and private state"
    );
    let accepted = library::apply(
        &library.hito,
        &library.actor,
        vec![Action::AudiobookBookmarkCreate {
            audiobook_id: library.playlist,
            bookmark_id,
            entry_id: library.entries[0],
            recording_id: library.recordings[0],
            position_ms: 250,
            label: "x".into(),
        }],
    )
    .await;
    let retained_book = book(&library).await;
    let retained_events = events(&library).await;
    let bookmarks = retained_book["bookmarks"].as_array().unwrap();
    assert!(
        bookmarks.len() == 1
            && bookmarks[0]["id"] == bookmark_id.to_string()
            && bookmarks[0]["label"] == "x"
            && bookmarks[0]["position_ms"] == 250,
        "A valid bounded marker must retain its exact independent bookmark identity and position"
    );
    let stored = retained_events["operations"].as_array().unwrap();
    let original = initial_events["operations"].as_array().unwrap();
    assert!(
        stored.len() == original.len() + 1
            && stored[..original.len()] == original[..]
            && stored.last().unwrap()["operation"]["id"] == accepted[0].to_string(),
        "The bounded marker must publish exactly one accepted operation after the rejected attempt"
    );
    let second_operation = Uuid::new_v4();
    assert!(
        second_operation != first_operation,
        "The existing-bookmark attempt must use a fresh operation rather than exact-operation replay"
    );
    let rejected = library
        .hito
        .collection
        .apply(
            &library.actor,
            http_sized_batch(&library, bookmark_id, second_operation, &padded),
        )
        .await;
    assert!(
        matches!(rejected, Err(Error::Invalid(_))),
        "An existing bookmark must not admit a fresh oversized raw sync payload merely because its trimmed label agrees"
    );
    assert!(
        book(&library).await == retained_book && events(&library).await == retained_events,
        "Rejected existing-bookmark padding must neither duplicate evidence nor append an oversized event"
    );
    assert!(
        library.snapshot().await == before,
        "Both rejected raw labels and the bounded bookmark must preserve the complete private snapshot"
    );
    library.assert_no_downloads().await;
    library.assert_originals_unchanged();
    let reopened = library.reopen().await;
    assert!(
        book(&reopened).await == retained_book && events(&reopened).await == retained_events,
        "Only the valid bounded bookmark and its accepted event may survive reopening core"
    );
    assert!(
        reopened.snapshot().await == before,
        "Raw label enforcement must retain sources, observations, private edits, playlists, likes and saved progress"
    );
    reopened.assert_no_downloads().await;
    reopened.assert_originals_unchanged();
}

fn http_sized_batch(
    library: &library::Library,
    bookmark_id: Uuid,
    operation_id: Uuid,
    label: &str,
) -> SyncBatch {
    let device_id = "audiobook-label-limit-client";
    let operation = Operation {
        id: operation_id,
        action: Action::AudiobookBookmarkCreate {
            audiobook_id: library.playlist,
            bookmark_id,
            entry_id: library.entries[0],
            recording_id: library.recordings[0],
            position_ms: 250,
            label: label.into(),
        },
    };
    let encoded = serde_json::to_vec(&json!({
        "device_id": device_id,
        "operations": [&operation],
    }))
    .unwrap();
    assert!(
        encoded.len() <= 262_144 && encoded.len() > 250_000,
        "The exact oversized-label batch must still fit the current public HTTP body limit"
    );
    SyncBatch {
        device_id: device_id.into(),
        operations: vec![operation],
    }
}

async fn book(library: &library::Library) -> Value {
    serde_json::to_value(
        library
            .hito
            .collection
            .audiobook(&library.actor, &library.playlist.to_string())
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn events(library: &library::Library) -> Value {
    serde_json::to_value(
        library
            .hito
            .collection
            .events(&library.actor, 0)
            .await
            .unwrap(),
    )
    .unwrap()
}
