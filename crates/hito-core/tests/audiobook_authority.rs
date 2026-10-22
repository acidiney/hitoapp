#[path = "fixtures/audiobooks_library.rs"]
mod library;
#[path = "fixtures/audiobooks_peer.rs"]
mod peer;
use hito_core::test_support::support;

use hito_core::{Action, Actor, Error, Operation, SyncBatch};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn private_book_authority_survives_replay_and_chapter_reordering() {
    let library = library::Library::prepare().await;
    let peer = peer::Peer::prepare(&library).await;
    let operations = [Uuid::new_v4(), Uuid::new_v4()];
    let bookmark = Uuid::new_v4();
    let receipt = library
        .hito
        .collection
        .apply(
            &library.actor,
            original_batch(&library, operations, bookmark),
        )
        .await
        .unwrap();
    assert!(
        receipt.acknowledged.as_slice() == operations.as_slice(),
        "The original private grouping and bookmark must be acknowledged exactly"
    );
    let before = library.snapshot().await;
    let other_before = peer.snapshot(&library).await;
    assert!(
        other_before["book"]["chapters"][0]["recording_id"] == library.recordings[0].to_string()
            && other_before["book"]["chapters"][0]["entry_id"] == peer.entry.to_string()
            && peer.entry != library.entries[0],
        "The peer must have its own chapter occurrence for the same accessible recording"
    );
    assert!(
        other_before["metadata"][0]["title"]["personal"]
            != before["metadata"][0]["title"]["personal"],
        "Both owners must retain independent personal metadata for the shared recording"
    );
    let book = library
        .hito
        .collection
        .audiobook(&library.actor, &library.playlist.to_string())
        .await
        .unwrap();
    assert!(
        book.bookmarks.len() == 1,
        "The original private book must contain one bookmark"
    );
    let retained_book = serde_json::to_value(book).unwrap();
    let retained_events = serde_json::to_value(
        library
            .hito
            .collection
            .events(&library.actor, 0)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        matches!(
            library
                .hito
                .collection
                .audiobook(&peer.actor, &library.playlist.to_string())
                .await,
            Err(Error::NotFound)
        ),
        "Shared recording access must not expose another account's private audiobook"
    );
    deny(
        &library,
        &peer.actor,
        operations[0],
        Action::AudiobookCreate {
            playlist_id: library.playlist,
        },
    )
    .await;
    deny(
        &library,
        &peer.actor,
        operations[1],
        Action::AudiobookBookmarkCreate {
            audiobook_id: library.playlist,
            bookmark_id: bookmark,
            entry_id: library.entries[0],
            recording_id: library.recordings[0],
            position_ms: 250,
            label: "Private return point".into(),
        },
    )
    .await;
    deny(
        &library,
        &peer.actor,
        operations[1],
        Action::AudiobookBookmarkCreate {
            audiobook_id: peer.book,
            bookmark_id: bookmark,
            entry_id: library.entries[0],
            recording_id: library.recordings[0],
            position_ms: 250,
            label: "Private return point".into(),
        },
    )
    .await;
    assert!(
        library.snapshot().await == before && peer.snapshot(&library).await == other_before,
        "Denied book, occurrence and bookmark identities must leave both private collections unchanged"
    );
    assert!(
        serde_json::to_value(
            library
                .hito
                .collection
                .audiobook(&library.actor, &library.playlist.to_string())
                .await
                .unwrap()
        )
        .unwrap()
            == retained_book,
        "Denied cross-account operations must not rewrite or duplicate the owner's bookmark"
    );
    assert!(
        serde_json::to_value(
            library
                .hito
                .collection
                .events(&library.actor, 0)
                .await
                .unwrap()
        )
        .unwrap()
            == retained_events,
        "Denied operations must not publish owner events"
    );
    let replay = library
        .hito
        .collection
        .apply(
            &library.actor,
            original_batch(&library, operations, bookmark),
        )
        .await
        .unwrap();
    assert!(
        replay.acknowledged.as_slice() == operations.as_slice(),
        "Exact operation replay must return the original acknowledgement"
    );
    assert!(
        serde_json::to_value(
            library
                .hito
                .collection
                .audiobook(&library.actor, &library.playlist.to_string())
                .await
                .unwrap()
        )
        .unwrap()
            == retained_book,
        "Exact replay must retain one bookmark and its original chapter anchor"
    );
    assert!(
        serde_json::to_value(
            library
                .hito
                .collection
                .events(&library.actor, 0)
                .await
                .unwrap()
        )
        .unwrap()
            == retained_events,
        "Exact replay must not append duplicate grouping or bookmark events"
    );
    assert!(
        library.snapshot().await == before,
        "Exact replay must preserve existing musical and private state"
    );
    let moved = library::apply(
        &library.hito,
        &library.actor,
        vec![Action::EntryMove {
            playlist_id: library.playlist,
            entry_id: library.entries[0],
            position: 30,
        }],
    )
    .await;
    let reordered = library
        .hito
        .collection
        .audiobook(&library.actor, &library.playlist.to_string())
        .await
        .unwrap();
    assert!(
        reordered.chapters.len() == 2
            && reordered.chapters[0].entry_id == library.entries[1].to_string()
            && reordered.chapters[1].entry_id == library.entries[0].to_string()
            && reordered.chapters[0].recording_id.as_deref()
                == Some(library.recordings[1].to_string().as_str())
            && reordered.chapters[1].recording_id.as_deref()
                == Some(library.recordings[0].to_string().as_str()),
        "Reordering must move chapter occurrences without changing their recording identities"
    );
    assert!(
        serde_json::to_value(&reordered.bookmarks).unwrap() == retained_book["bookmarks"],
        "A bookmark must stay on the same occurrence, recording and position after reordering"
    );
    let mut expected = before;
    expected["chapters"]["entries"][0]["position"] = json!(30);
    expected["chapters"]["entries"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert!(
        library.snapshot().await == expected,
        "Only the explicitly moved chapter position and order may change in the complete private snapshot"
    );
    let events = library
        .hito
        .collection
        .events(&library.actor, 0)
        .await
        .unwrap();
    let previous = retained_events["operations"].as_array().unwrap();
    let encoded = serde_json::to_value(&events.operations).unwrap();
    assert!(
        events.operations.len() == previous.len() + 1
            && encoded.as_array().unwrap()[..previous.len()] == previous[..]
            && encoded.as_array().unwrap().last().unwrap()["operation"]["id"]
                == moved[0].to_string(),
        "The chapter move must add exactly one new event while retaining the prior accepted operations"
    );
    let retained_reordered = serde_json::to_value(reordered).unwrap();
    library.assert_no_downloads().await;
    peer.assert_no_downloads(&library).await;
    library.assert_originals_unchanged();
    let reopened = library.reopen().await;
    assert!(
        serde_json::to_value(
            reopened
                .hito
                .collection
                .audiobook(&reopened.actor, &reopened.playlist.to_string())
                .await
                .unwrap()
        )
        .unwrap()
            == retained_reordered,
        "The reordered book and original bookmark anchor must survive reopening core"
    );
    assert!(
        reopened.snapshot().await == expected && peer.snapshot(&reopened).await == other_before,
        "Reopening must retain both independent private collections without cross-account changes"
    );
    reopened.assert_no_downloads().await;
    peer.assert_no_downloads(&reopened).await;
    reopened.assert_originals_unchanged();
}

fn original_batch(library: &library::Library, ids: [Uuid; 2], bookmark_id: Uuid) -> SyncBatch {
    SyncBatch {
        device_id: "audiobook-replay-client".into(),
        operations: vec![
            Operation {
                id: ids[0],
                action: Action::AudiobookCreate {
                    playlist_id: library.playlist,
                },
            },
            Operation {
                id: ids[1],
                action: Action::AudiobookBookmarkCreate {
                    audiobook_id: library.playlist,
                    bookmark_id,
                    entry_id: library.entries[0],
                    recording_id: library.recordings[0],
                    position_ms: 250,
                    label: "Private return point".into(),
                },
            },
        ],
    }
}

async fn deny(library: &library::Library, actor: &Actor, id: Uuid, action: Action) {
    let denied = library
        .hito
        .collection
        .apply(
            actor,
            SyncBatch {
                device_id: "audiobook-denied-client".into(),
                operations: vec![Operation { id, action }],
            },
        )
        .await;
    assert!(
        matches!(denied, Err(Error::NotFound)),
        "Known private identifiers must not authorize another account's book or chapter occurrence"
    );
}
