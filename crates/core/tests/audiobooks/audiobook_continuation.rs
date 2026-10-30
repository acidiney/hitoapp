#[path = "../fixtures/audiobooks_library.rs"]
mod library;
use hito_core::test_support::support;

use hito_core::{Action, Operation, SyncBatch};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn continuation_retains_an_explicit_duplicate_chapter_and_existing_recording_progress() {
    let library = library::Library::prepare().await;
    let duplicate = Uuid::new_v4();
    library::apply(
        &library.hito,
        &library.actor,
        vec![
            Action::EntryInsert {
                playlist_id: library.playlist,
                entry_id: duplicate,
                recording_id: library.recordings[0],
                position: 30,
            },
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
        ],
    )
    .await;
    let book_id = library.playlist.to_string();
    let recordings = library.recordings.map(|id| id.to_string());
    let before = library.snapshot().await;
    let book = library
        .hito
        .collection
        .audiobook(&library.actor, &book_id)
        .await
        .unwrap();
    assert!(
        book.chapters.len() == 3
            && book.chapters[0].entry_id == library.entries[0].to_string()
            && book.chapters[1].entry_id == library.entries[1].to_string()
            && book.chapters[2].entry_id == duplicate.to_string()
            && book.chapters[0].recording_id.as_deref() == Some(recordings[0].as_str())
            && book.chapters[1].recording_id.as_deref() == Some(recordings[1].as_str())
            && book.chapters[2].recording_id.as_deref() == Some(recordings[0].as_str()),
        "The owned book must retain distinct A, B, A chapter occurrences in accepted order"
    );
    let before_book = serde_json::to_value(book).unwrap();
    assert!(
        before["metadata"][0]["title"]["personal"] == library::PERSONAL_TITLE
            && before["metadata"][0]["artist"]["personal"] == library::PERSONAL_ARTIST
            && before_book["chapters"][1]["liked"] == true,
        "The preservation fixture must retain private chapter edits and an independent like"
    );
    assert!(
        before_book["continuation"].is_null(),
        "Existing recording progress must not invent a selected chapter"
    );
    assert_eq!(
        before["progress"],
        json!([
            {"recording_id": library.recordings[0], "position_ms": 100, "speed": 1.5},
            {"recording_id": library.recordings[1], "position_ms": 650, "speed": 1.25}
        ]),
        "Both recordings must begin with independent saved positions and speeds"
    );
    let before_events = serde_json::to_value(
        library
            .hito
            .collection
            .events(&library.actor, 0)
            .await
            .unwrap(),
    )
    .unwrap();
    let chapter = json!({
        "audiobook_id": library.playlist,
        "entry_id": duplicate,
        "recording_id": library.recordings[0]
    });
    let chapter_action = json!({
        "kind": "audiobook_chapter_set",
        "chapter": chapter
    });
    let action = serde_json::from_value::<Action>(chapter_action.clone())
        .expect("The public collection contract must accept an explicit audiobook chapter anchor");
    assert_eq!(
        serde_json::to_value(&action).unwrap(),
        chapter_action,
        "The explicit chapter action must retain its exact public synchronization payload"
    );
    let operations = [Uuid::new_v4(), Uuid::new_v4()];
    let batch = SyncBatch {
        device_id: "audiobook-continuation-client".into(),
        operations: vec![
            Operation {
                id: operations[0],
                action: Action::ProgressSet {
                    recording_id: library.recordings[0],
                    position_ms: 35_125,
                    speed: 1.25,
                },
            },
            Operation {
                id: operations[1],
                action,
            },
        ],
    };
    let submitted = serde_json::to_value(&batch.operations).unwrap();
    let receipt = library
        .hito
        .collection
        .apply(&library.actor, batch)
        .await
        .unwrap();
    assert_eq!(
        receipt.acknowledged.as_slice(),
        operations.as_slice(),
        "The recording checkpoint and explicit chapter must commit in one acknowledged batch"
    );
    let continuation = json!({
        "chapter": chapter,
        "progress": {
            "recording_id": library.recordings[0],
            "position_ms": 35_125,
            "speed": 1.25
        }
    });
    let mut expected_book = before_book;
    expected_book["continuation"] = continuation;
    assert_eq!(
        serde_json::to_value(
            library
                .hito
                .collection
                .audiobook(&library.actor, &book_id)
                .await
                .unwrap()
        )
        .unwrap(),
        expected_book,
        "Continuation must expose the second A occurrence and its existing recording progress without changing the book"
    );
    let later = library::apply(
        &library.hito,
        &library.actor,
        vec![Action::ProgressSet {
            recording_id: library.recordings[1],
            position_ms: 48_000,
            speed: 0.75,
        }],
    )
    .await;
    assert_eq!(
        serde_json::to_value(
            library
                .hito
                .collection
                .audiobook(&library.actor, &book_id)
                .await
                .unwrap()
        )
        .unwrap(),
        expected_book,
        "A newer ordinary recording checkpoint must not move the explicit chapter anchor"
    );
    let retained = library.snapshot().await;
    assert_eq!(
        retained["progress"],
        json!([
            {"recording_id": library.recordings[0], "position_ms": 35_125, "speed": 1.25},
            {"recording_id": library.recordings[1], "position_ms": 48_000, "speed": 0.75}
        ]),
        "The existing per-recording capability must expose exactly the two accepted checkpoints"
    );
    assert_eq!(
        preserved_musical_state(retained.clone()),
        preserved_musical_state(before),
        "Continuation must preserve private edits, likes, chapter entries, sources and unrelated musical state"
    );
    let retained_events = serde_json::to_value(
        library
            .hito
            .collection
            .events(&library.actor, 0)
            .await
            .unwrap(),
    )
    .unwrap();
    let previous = before_events["operations"].as_array().unwrap();
    let events = retained_events["operations"].as_array().unwrap();
    assert!(
        events.len() == previous.len() + 3 && events[..previous.len()] == previous[..],
        "Only the two checkpoint operations and explicit chapter operation may extend the accepted event prefix"
    );
    for (event, operation) in events[previous.len()..previous.len() + 2]
        .iter()
        .zip(submitted.as_array().unwrap())
    {
        assert!(
            event["device_id"] == "audiobook-continuation-client"
                && event["operation"] == *operation,
            "The atomic checkpoint batch must retain its exact client identity and operation payloads"
        );
    }
    assert_eq!(
        events.last().unwrap()["operation"],
        json!({
            "id": later[0],
            "action": {
                "kind": "progress_set",
                "recording_id": library.recordings[1],
                "position_ms": 48_000,
                "speed": 0.75
            }
        }),
        "The final event must remain the ordinary B recording checkpoint"
    );
    library.assert_no_downloads().await;
    library.assert_originals_unchanged();

    let reopened = library.reopen().await;
    assert_eq!(
        serde_json::to_value(
            reopened
                .hito
                .collection
                .audiobook(&reopened.actor, &book_id)
                .await
                .unwrap()
        )
        .unwrap(),
        expected_book,
        "Reopening must retain the exact duplicate chapter anchor and recording progress projection"
    );
    assert_eq!(
        reopened.snapshot().await,
        retained,
        "Reopening must preserve every musical and private value including both accepted recording checkpoints"
    );
    assert_eq!(
        serde_json::to_value(
            reopened
                .hito
                .collection
                .events(&reopened.actor, 0)
                .await
                .unwrap()
        )
        .unwrap(),
        retained_events,
        "Reopening and continuation reads must not append or rewrite collection events"
    );
    reopened.assert_no_downloads().await;
    reopened.assert_originals_unchanged();
}

fn preserved_musical_state(mut snapshot: Value) -> Value {
    snapshot.as_object_mut().unwrap().remove("progress");
    let history = snapshot["activity"]["history"].as_array_mut().unwrap();
    for entry in history.iter_mut() {
        let entry = entry.as_object_mut().unwrap();
        for field in ["position_ms", "speed", "updated_at"] {
            entry.remove(field);
        }
    }
    history.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    snapshot
}
