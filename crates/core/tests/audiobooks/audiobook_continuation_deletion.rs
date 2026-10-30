#[path = "../fixtures/audiobooks_deleted.rs"]
mod deleted;
#[path = "../fixtures/audiobooks_library.rs"]
mod library;
#[path = "../fixtures/audiobooks_peer.rs"]
mod peer;
use hito_core::test_support::support;

use hito_core::{Action, AudiobookChapterAnchor, Error};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn chapter_and_book_deletion_win_over_delayed_continuation_without_blocking_independent_work()
{
    let library = library::Library::prepare().await;
    let peer = peer::Peer::prepare(&library).await;
    let initial = library.snapshot().await;
    let alternate =
        Uuid::parse_str(initial["unrelated"]["playlist"]["id"].as_str().unwrap()).unwrap();
    let alternate_entry = Uuid::parse_str(
        initial["unrelated"]["entries"][0]["entry_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let duplicate = Uuid::new_v4();
    let never_grouped = Uuid::new_v4();
    let never_grouped_entry = Uuid::new_v4();
    let chapter = AudiobookChapterAnchor {
        audiobook_id: library.playlist,
        entry_id: duplicate,
        recording_id: library.recordings[0],
    };
    let alternate_chapter = AudiobookChapterAnchor {
        audiobook_id: alternate,
        entry_id: alternate_entry,
        ..chapter
    };
    let ungrouped_chapter = AudiobookChapterAnchor {
        audiobook_id: never_grouped,
        entry_id: never_grouped_entry,
        ..chapter
    };
    assert!(
        peer.entry != duplicate && peer.entry != library.entries[0],
        "The neighboring account must retain an independent occurrence for the same recording"
    );
    deleted::accept(
        &library,
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
            Action::AudiobookCreate {
                playlist_id: alternate,
            },
            Action::PlaylistCreate {
                playlist_id: never_grouped,
                name: "Private playlist never grouped before deletion".into(),
            },
            Action::EntryInsert {
                playlist_id: never_grouped,
                entry_id: never_grouped_entry,
                recording_id: library.recordings[0],
                position: 0,
            },
            Action::ProgressSet {
                recording_id: library.recordings[0],
                position_ms: 35_125,
                speed: 1.25,
            },
            Action::AudiobookChapterSet { chapter },
        ],
    )
    .await;
    let mut expected_book = read_book(&library, library.playlist).await;
    assert_eq!(
        expected_book["continuation"],
        json!({
            "chapter": chapter,
            "progress": {
                "recording_id": library.recordings[0],
                "position_ms": 35_125,
                "speed": 1.25
            }
        })
    );
    assert!(read_book(&library, alternate).await["continuation"].is_null());
    assert!(matches!(
        library
            .hito
            .collection
            .audiobook(&library.actor, &never_grouped.to_string())
            .await,
        Err(Error::NotFound)
    ));
    let peer_before = peer.snapshot(&library).await;
    deleted::accept(
        &library,
        vec![
            Action::EntryRemove {
                playlist_id: library.playlist,
                entry_id: duplicate,
            },
            Action::EntryRemove {
                playlist_id: alternate,
                entry_id: alternate_entry,
            },
        ],
    )
    .await;
    expected_book["chapters"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["entry_id"] != duplicate.to_string());
    assert_eq!(
        read_book(&library, library.playlist).await,
        expected_book,
        "Removing the selected second A must retain its exact unavailable anchor without substituting the surviving first A"
    );
    assert!(
        expected_book["chapters"][0]["entry_id"] == library.entries[0].to_string()
            && expected_book["chapters"][0]["recording_id"] == library.recordings[0].to_string()
            && expected_book["chapters"][0]["available"] == true,
        "The first A occurrence must remain playable while the selected second A is unavailable"
    );
    let expected_alternate = read_book(&library, alternate).await;
    assert!(expected_alternate["continuation"].is_null());
    let mut expected = deleted::musical(&library, alternate).await;
    deleted::accept(
        &library,
        vec![
            Action::AudiobookChapterSet { chapter },
            Action::AudiobookChapterSet {
                chapter: alternate_chapter,
            },
            Action::LikeSet {
                recording_id: library.recordings[1],
                liked: false,
            },
            Action::ProgressSet {
                recording_id: library.recordings[1],
                position_ms: 8_125,
                speed: 1.75,
            },
        ],
    )
    .await;
    expected_book["chapters"][1]["liked"] = json!(false);
    assert_eq!(
        read_book(&library, library.playlist).await,
        expected_book,
        "A fresh delayed anchor for a retired chapter must be a no-op while independent work commits"
    );
    assert_eq!(
        read_book(&library, alternate).await,
        expected_alternate,
        "Removing an occurrence before its first selection must prevent a delayed action from creating an anchor"
    );
    for recording in expected["recordings"]["items"].as_array_mut().unwrap() {
        if recording["id"] == library.recordings[1].to_string() {
            recording["liked"] = json!(false);
        }
    }
    let history = expected["activity"]["history"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == library.recordings[1].to_string())
        .unwrap();
    history["liked"] = json!(false);
    history["position_ms"] = json!(8_125);
    history["speed"] = json!(1.75);
    expected["progress"] = json!([
        {"recording_id": library.recordings[0], "position_ms": 35_125, "speed": 1.25},
        {"recording_id": library.recordings[1], "position_ms": 8_125, "speed": 1.75}
    ]);
    assert_eq!(
        stable_history(deleted::musical(&library, alternate).await),
        stable_history(expected.clone()),
        "Only the independent B like and recording checkpoint may change private musical state"
    );
    assert_eq!(peer.snapshot(&library).await, peer_before);
    let retired_events = deleted::events(&library).await;
    let retired_musical = deleted::musical(&library, alternate).await;
    let library = library.reopen().await;
    assert_eq!(read_book(&library, library.playlist).await, expected_book);
    assert_eq!(read_book(&library, alternate).await, expected_alternate);
    assert_eq!(deleted::events(&library).await, retired_events);
    assert_eq!(deleted::musical(&library, alternate).await, retired_musical);
    assert_eq!(peer.snapshot(&library).await, peer_before);

    let first_name = "Private independent work in the deletion batch";
    deleted::accept(
        &library,
        vec![
            Action::PlaylistDelete {
                playlist_id: library.playlist,
            },
            Action::AudiobookChapterSet { chapter },
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            Action::PlaylistDelete {
                playlist_id: never_grouped,
            },
            Action::AudiobookChapterSet {
                chapter: ungrouped_chapter,
            },
            Action::AudiobookCreate {
                playlist_id: never_grouped,
            },
            Action::PlaylistRename {
                playlist_id: alternate,
                name: first_name.into(),
            },
        ],
    )
    .await;
    expected = deleted::expected_after_deletion(&expected, library.playlist, alternate, first_name);
    expected["playlists"]
        .as_array_mut()
        .unwrap()
        .retain(|playlist| playlist["id"] != never_grouped.to_string());
    assert_deleted(&library, never_grouped).await;
    let final_name = "Private independent work after committed deletion";
    deleted::accept(
        &library,
        vec![
            Action::AudiobookChapterSet { chapter },
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            Action::AudiobookChapterSet {
                chapter: ungrouped_chapter,
            },
            Action::AudiobookCreate {
                playlist_id: never_grouped,
            },
            Action::PlaylistRename {
                playlist_id: alternate,
                name: final_name.into(),
            },
        ],
    )
    .await;
    deleted::rename(&mut expected, alternate, final_name);
    assert_deleted(&library, never_grouped).await;
    let retained_events = deleted::events(&library).await;
    deleted::deny(
        &library,
        &peer.actor,
        Action::AudiobookChapterSet { chapter },
    )
    .await;
    assert_eq!(deleted::events(&library).await, retained_events);
    assert_eq!(peer.snapshot(&library).await, peer_before);
    let retained_musical = deleted::musical(&library, alternate).await;
    assert_eq!(
        stable_history(retained_musical.clone()),
        stable_history(expected.clone()),
        "Deleted-book no-ops must preserve private progress, edits, sources and the independent committed work"
    );
    library.assert_no_downloads().await;
    peer.assert_no_downloads(&library).await;
    library.assert_originals_unchanged();

    let reopened = library.reopen().await;
    assert_deleted(&reopened, never_grouped).await;
    assert_eq!(
        deleted::musical(&reopened, alternate).await,
        retained_musical,
        "Reopening must preserve exact progress, activity timestamps, private state and independent work after deletion"
    );
    assert_eq!(deleted::events(&reopened).await, retained_events);
    assert_eq!(peer.snapshot(&reopened).await, peer_before);
    assert!(read_book(&reopened, alternate).await["continuation"].is_null());
    reopened.assert_no_downloads().await;
    peer.assert_no_downloads(&reopened).await;
    reopened.assert_originals_unchanged();
}

async fn read_book(library: &library::Library, id: Uuid) -> Value {
    serde_json::to_value(
        library
            .hito
            .collection
            .audiobook(&library.actor, &id.to_string())
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn assert_deleted(library: &library::Library, never_grouped: Uuid) {
    deleted::assert_deleted(library).await;
    let id = never_grouped.to_string();
    assert!(matches!(
        library.hito.collection.playlist(&library.actor, &id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        library.hito.collection.audiobook(&library.actor, &id).await,
        Err(Error::NotFound)
    ));
}

fn stable_history(mut snapshot: Value) -> Value {
    let history = snapshot["activity"]["history"].as_array_mut().unwrap();
    for entry in history.iter_mut() {
        entry.as_object_mut().unwrap().remove("updated_at");
    }
    history.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    snapshot
}
