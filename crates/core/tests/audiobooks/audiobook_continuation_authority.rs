#[path = "../fixtures/audiobooks_library.rs"]
mod library;
#[path = "../fixtures/audiobooks_peer.rs"]
mod peer;
use hito_core::test_support::support;

use hito_core::{Action, Actor, AudiobookChapterAnchor, Credentials, Error, Operation, SyncBatch};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn continuation_requires_private_book_ownership_and_current_recording_access() {
    let library = library::Library::prepare().await;
    let peer = peer::Peer::prepare(&library).await;
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
    let chapter = AudiobookChapterAnchor {
        audiobook_id: library.playlist,
        entry_id: duplicate,
        recording_id: library.recordings[0],
    };
    let checkpoint = SyncBatch {
        device_id: "audiobook-continuation-authority-client".into(),
        operations: vec![
            Operation {
                id: Uuid::new_v4(),
                action: Action::ProgressSet {
                    recording_id: library.recordings[0],
                    position_ms: 35_125,
                    speed: 1.25,
                },
            },
            Operation {
                id: Uuid::new_v4(),
                action: Action::AudiobookChapterSet { chapter },
            },
        ],
    };
    let device = checkpoint.device_id.clone();
    let operations = serde_json::to_value(&checkpoint.operations).unwrap();
    let accepted = library
        .hito
        .collection
        .apply(&library.actor, checkpoint)
        .await
        .unwrap();
    let before = owner_snapshot(&library).await;
    let peer_before = peer.snapshot(&library).await;
    assert_eq!(
        before["book"]["continuation"],
        json!({
            "chapter": chapter,
            "progress": {
                "recording_id": library.recordings[0],
                "position_ms": 35_125,
                "speed": 1.25
            }
        })
    );
    assert!(
        peer_before["book"]["chapters"][0]["recording_id"] == library.recordings[0].to_string()
            && peer_before["book"]["chapters"][0]["entry_id"] == peer.entry.to_string()
            && peer.entry != duplicate
            && peer.entry != library.entries[0]
            && peer_before["progress"][0]["position_ms"] == 500
            && peer_before["progress"][0]["speed"] == 1.75,
        "The foreign account must have its own private book and progress for the same accessible recording"
    );
    assert!(
        matches!(
            library
                .hito
                .collection
                .audiobook(&peer.actor, &library.playlist.to_string())
                .await,
            Err(Error::NotFound)
        ),
        "Shared recording access must not expose another account's continuation or book"
    );
    let unrelated = Uuid::parse_str(
        before["musical"]["unrelated"]["playlist"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let unrelated_entry = Uuid::parse_str(
        before["musical"]["unrelated"]["entries"][0]["entry_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    for (actor, editable, target) in [
        (&peer.actor, peer.book, chapter),
        (
            &library.actor,
            unrelated,
            AudiobookChapterAnchor {
                entry_id: library.entries[1],
                ..chapter
            },
        ),
        (
            &library.actor,
            unrelated,
            AudiobookChapterAnchor {
                recording_id: library.recordings[1],
                ..chapter
            },
        ),
        (
            &library.actor,
            unrelated,
            AudiobookChapterAnchor {
                audiobook_id: unrelated,
                entry_id: unrelated_entry,
                ..chapter
            },
        ),
        (
            &library.actor,
            unrelated,
            AudiobookChapterAnchor {
                audiobook_id: Uuid::new_v4(),
                ..chapter
            },
        ),
        (
            &library.actor,
            unrelated,
            AudiobookChapterAnchor {
                entry_id: Uuid::new_v4(),
                ..chapter
            },
        ),
        (
            &library.actor,
            unrelated,
            AudiobookChapterAnchor {
                recording_id: Uuid::new_v4(),
                ..chapter
            },
        ),
    ] {
        deny_after_valid_rename(&library, actor, editable, target).await;
        assert_eq!(
            owner_snapshot(&library).await,
            before,
            "A denied anchor must roll back prior valid work and preserve the owner's book, progress and events"
        );
        assert_eq!(
            peer.snapshot(&library).await,
            peer_before,
            "Denied cross-account or mismatched targets must preserve the neighboring account"
        );
    }

    let session = library
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let admin = library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let shared = library
        .hito
        .catalog
        .libraries(&library.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|item| !item.personal)
        .unwrap();
    library
        .hito
        .identity
        .set_library_access(&admin, &shared.id, &library.actor.user().id, false)
        .await
        .unwrap();
    let revoked = serde_json::to_value(
        library
            .hito
            .collection
            .audiobook(&library.actor, &library.playlist.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        revoked["continuation"],
        json!({"chapter": chapter, "progress": null}),
        "Revocation must retain the explicit anchor while exposing no saved position or speed"
    );
    assert!(
        matches!(
            library
                .hito
                .collection
                .saved_playback(&library.actor, &[library.recordings[0]])
                .await,
            Err(Error::NotFound)
        ),
        "The ordinary progress capability must enforce the same current recording authority"
    );
    deny_after_valid_rename(
        &library,
        &library.actor,
        unrelated,
        AudiobookChapterAnchor {
            entry_id: library.entries[0],
            ..chapter
        },
    )
    .await;
    assert_eq!(
        serde_json::to_value(
            library
                .hito
                .collection
                .events(&library.actor, 0)
                .await
                .unwrap()
        )
        .unwrap(),
        before["events"],
        "A write denied after revocation must retain the exact accepted event and receipt history"
    );
    library
        .hito
        .identity
        .set_library_access(&admin, &shared.id, &library.actor.user().id, true)
        .await
        .unwrap();
    assert_eq!(
        owner_snapshot(&library).await,
        before,
        "Restored authority must recover the original progress, anchor, private edits and unrelated state"
    );
    assert_eq!(peer.snapshot(&library).await, peer_before);
    let replay = library
        .hito
        .collection
        .apply(
            &library.actor,
            SyncBatch {
                device_id: device,
                operations: serde_json::from_value(operations).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.acknowledged, accepted.acknowledged);
    assert_eq!(owner_snapshot(&library).await, before);
    assert_eq!(peer.snapshot(&library).await, peer_before);
    library.assert_no_downloads().await;
    peer.assert_no_downloads(&library).await;
    library.assert_originals_unchanged();

    let reopened = library.reopen().await;
    assert_eq!(
        owner_snapshot(&reopened).await,
        before,
        "Reopening after restored authority must preserve the exact owner book, progress and events"
    );
    assert_eq!(peer.snapshot(&reopened).await, peer_before);
    reopened.assert_no_downloads().await;
    peer.assert_no_downloads(&reopened).await;
    reopened.assert_originals_unchanged();
}

async fn deny_after_valid_rename(
    library: &library::Library,
    actor: &Actor,
    editable: Uuid,
    chapter: AudiobookChapterAnchor,
) {
    let result = library
        .hito
        .collection
        .apply(
            actor,
            SyncBatch {
                device_id: "audiobook-continuation-denied-client".into(),
                operations: vec![
                    Operation {
                        id: Uuid::new_v4(),
                        action: Action::PlaylistRename {
                            playlist_id: editable,
                            name: "Private rename that must roll back".into(),
                        },
                    },
                    Operation {
                        id: Uuid::new_v4(),
                        action: Action::AudiobookChapterSet { chapter },
                    },
                ],
            },
        )
        .await;
    assert!(
        matches!(result, Err(Error::NotFound)),
        "Foreign, ungrouped, missing, mismatched and revoked targets must share the same denied result"
    );
}

async fn owner_snapshot(library: &library::Library) -> Value {
    json!({
        "book": library.hito.collection.audiobook(&library.actor, &library.playlist.to_string()).await.unwrap(),
        "musical": library.snapshot().await,
        "events": library.hito.collection.events(&library.actor, 0).await.unwrap()
    })
}
