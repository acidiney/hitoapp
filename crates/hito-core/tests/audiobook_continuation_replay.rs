#[path = "fixtures/audiobooks_library.rs"]
mod library;
use hito_core::test_support::support;

use hito_core::{Action, AudiobookChapterAnchor, Operation, SyncBatch};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn old_checkpoint_replay_preserves_the_selected_duplicate_after_reordering_and_progress_changes()
 {
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
    let before = library.snapshot().await;
    let mut expected_book = read_book(&library).await;
    let first = chapter_checkpoint(&library, library.entries[0], 1_250, 1.5);
    let first_device = first.device_id.clone();
    let first_operations = serde_json::to_value(&first.operations).unwrap();
    let first_ids: Vec<Uuid> = first
        .operations
        .iter()
        .map(|operation| operation.id)
        .collect();
    let first_receipt = library
        .hito
        .collection
        .apply(&library.actor, first)
        .await
        .unwrap();
    assert_eq!(first_receipt.acknowledged, first_ids);
    expected_book["continuation"] = json!({
        "chapter": {
            "audiobook_id": library.playlist,
            "entry_id": library.entries[0],
            "recording_id": library.recordings[0]
        },
        "progress": {
            "recording_id": library.recordings[0],
            "position_ms": 1_250,
            "speed": 1.5
        }
    });
    assert_eq!(
        read_book(&library).await,
        expected_book,
        "The first checkpoint must select the first A occurrence explicitly"
    );
    let second = chapter_checkpoint(&library, duplicate, 35_125, 1.25);
    let second_ids: Vec<Uuid> = second
        .operations
        .iter()
        .map(|operation| operation.id)
        .collect();
    let second_receipt = library
        .hito
        .collection
        .apply(&library.actor, second)
        .await
        .unwrap();
    assert_eq!(second_receipt.acknowledged, second_ids);
    expected_book["continuation"]["chapter"]["entry_id"] = json!(duplicate);
    expected_book["continuation"]["progress"]["position_ms"] = json!(35_125);
    expected_book["continuation"]["progress"]["speed"] = json!(1.25);
    assert_eq!(
        read_book(&library).await,
        expected_book,
        "The newer checkpoint must select the distinct second A occurrence and its current recording progress"
    );
    library::apply(
        &library.hito,
        &library.actor,
        vec![
            Action::EntryMove {
                playlist_id: library.playlist,
                entry_id: library.entries[0],
                position: 40,
            },
            Action::EntryMove {
                playlist_id: library.playlist,
                entry_id: duplicate,
                position: 5,
            },
        ],
    )
    .await;
    expected_book["chapters"][0]["position"] = json!(40);
    expected_book["chapters"][2]["position"] = json!(5);
    expected_book["chapters"].as_array_mut().unwrap().swap(0, 2);
    assert_eq!(
        read_book(&library).await,
        expected_book,
        "Reordering must move the chapter occurrences while continuation retains the selected entry identity"
    );
    let before_replay = library.snapshot().await;
    let mut expected_musical = preserved_musical_state(before);
    expected_musical["chapters"]["entries"][0]["position"] = json!(40);
    expected_musical["chapters"]["entries"][2]["position"] = json!(5);
    expected_musical["chapters"]["entries"]
        .as_array_mut()
        .unwrap()
        .swap(0, 2);
    assert_eq!(
        preserved_musical_state(before_replay.clone()),
        expected_musical,
        "Both checkpoint selections and the explicit moves must preserve private edits, likes, sources and all unrelated musical state"
    );
    let before_replay_events = collection_events(&library).await;
    let replay_receipt = library
        .hito
        .collection
        .apply(
            &library.actor,
            SyncBatch {
                device_id: first_device,
                operations: serde_json::from_value(first_operations).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        replay_receipt.acknowledged, first_receipt.acknowledged,
        "Replaying the frozen original client, operation IDs and payloads must return the original acknowledgements"
    );
    assert_eq!(
        read_book(&library).await,
        expected_book,
        "Old checkpoint replay must not roll the selected duplicate, saved progress or chapter order back"
    );
    assert_eq!(
        library.snapshot().await,
        before_replay,
        "Exact replay must preserve every current musical and private value"
    );
    assert_eq!(
        collection_events(&library).await,
        before_replay_events,
        "Exact replay must not append, replace or reorder accepted collection events"
    );
    let ordinary = library::apply(
        &library.hito,
        &library.actor,
        vec![Action::ProgressSet {
            recording_id: library.recordings[0],
            position_ms: 42_125,
            speed: 1.75,
        }],
    )
    .await;
    expected_book["continuation"]["progress"]["position_ms"] = json!(42_125);
    expected_book["continuation"]["progress"]["speed"] = json!(1.75);
    assert_eq!(
        read_book(&library).await,
        expected_book,
        "Ordinary progress for the same recording must update continuation position and speed without moving the selected occurrence"
    );
    let retained = library.snapshot().await;
    assert_eq!(
        retained["progress"],
        json!([
            {"recording_id": library.recordings[0], "position_ms": 42_125, "speed": 1.75},
            {"recording_id": library.recordings[1], "position_ms": 650, "speed": 1.25}
        ]),
        "Continuation must use the same authoritative per-recording progress as ordinary playback"
    );
    assert_eq!(
        preserved_musical_state(retained.clone()),
        expected_musical,
        "The ordinary checkpoint must preserve the reordered book and all private musical state"
    );
    let retained_events = collection_events(&library).await;
    let previous = before_replay_events["operations"].as_array().unwrap();
    let events = retained_events["operations"].as_array().unwrap();
    assert!(
        events.len() == previous.len() + 1 && events[..previous.len()] == previous[..],
        "Only the ordinary same-recording checkpoint may extend the accepted event prefix"
    );
    assert_eq!(
        events.last().unwrap()["operation"],
        json!({
            "id": ordinary[0],
            "action": {
                "kind": "progress_set",
                "recording_id": library.recordings[0],
                "position_ms": 42_125,
                "speed": 1.75
            }
        })
    );
    library.assert_no_downloads().await;
    library.assert_originals_unchanged();

    let reopened = library.reopen().await;
    assert_eq!(
        read_book(&reopened).await,
        expected_book,
        "Reopening must preserve the selected duplicate, reordered chapters and ordinary recording progress"
    );
    assert_eq!(reopened.snapshot().await, retained);
    assert_eq!(collection_events(&reopened).await, retained_events);
    reopened.assert_no_downloads().await;
    reopened.assert_originals_unchanged();
}

fn chapter_checkpoint(
    library: &library::Library,
    entry_id: Uuid,
    position_ms: i64,
    speed: f64,
) -> SyncBatch {
    SyncBatch {
        device_id: "audiobook-continuation-replay-client".into(),
        operations: vec![
            Operation {
                id: Uuid::new_v4(),
                action: Action::ProgressSet {
                    recording_id: library.recordings[0],
                    position_ms,
                    speed,
                },
            },
            Operation {
                id: Uuid::new_v4(),
                action: Action::AudiobookChapterSet {
                    chapter: AudiobookChapterAnchor {
                        audiobook_id: library.playlist,
                        entry_id,
                        recording_id: library.recordings[0],
                    },
                },
            },
        ],
    }
}

async fn read_book(library: &library::Library) -> Value {
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

async fn collection_events(library: &library::Library) -> Value {
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
