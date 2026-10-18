#[path = "fixtures/audiobook_queue.rs"]
mod audiobook;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use hito_core::{Action, Error};
use hito_playback::{Player, PlayerCommand, PlayerError};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn accessible_recordings_do_not_authorize_foreign_or_invalid_audiobook_context() {
    let f = audiobook::Library::start().await;
    let [a, b] = f.recordings;
    let foreign = Uuid::new_v4();
    let foreign_entry = Uuid::new_v4();
    support::synchronize(
        &f.source.services,
        &f.source.actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: foreign,
                name: "Private foreign audiobook".into(),
            },
            Action::EntryInsert {
                playlist_id: foreign,
                entry_id: foreign_entry,
                recording_id: a,
                position: 0,
            },
            Action::AudiobookCreate {
                playlist_id: foreign,
            },
            Action::RecordingTitleSet {
                recording_id: a,
                title: "Private foreign recording title".into(),
            },
            Action::LikeSet {
                recording_id: a,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: a,
                position_ms: 444,
                speed: 0.75,
            },
        ],
    )
    .await;
    let ungrouped = Uuid::new_v4();
    let ungrouped_entry = Uuid::new_v4();
    let deleted = Uuid::new_v4();
    let deleted_entry = Uuid::new_v4();
    let removed_entry = Uuid::new_v4();
    support::synchronize(
        &f.source.services,
        &f.actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: ungrouped,
                name: "Private ordinary playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: ungrouped,
                entry_id: ungrouped_entry,
                recording_id: a,
                position: 0,
            },
            Action::PlaylistCreate {
                playlist_id: deleted,
                name: "Private deleted audiobook".into(),
            },
            Action::EntryInsert {
                playlist_id: deleted,
                entry_id: deleted_entry,
                recording_id: a,
                position: 0,
            },
            Action::AudiobookCreate {
                playlist_id: deleted,
            },
            Action::PlaylistDelete {
                playlist_id: deleted,
            },
            Action::EntryInsert {
                playlist_id: f.book,
                entry_id: removed_entry,
                recording_id: a,
                position: 3,
            },
            Action::EntryRemove {
                playlist_id: f.book,
                entry_id: removed_entry,
            },
            Action::ProgressSet {
                recording_id: a,
                position_ms: 313,
                speed: 1.25,
            },
            Action::ProgressSet {
                recording_id: b,
                position_ms: 17,
                speed: 1.5,
            },
            Action::AudiobookChapterSet {
                chapter: serde_json::from_value(f.anchors[0].clone()).unwrap(),
            },
        ],
    )
    .await;
    let before_private = private_state(&f, foreign, ungrouped, deleted).await;
    let (player, owner) = Player::start(f.source.services.clone());
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    let accepted = command(
        Uuid::new_v4(),
        lease,
        1,
        0,
        json!({
            "kind": "replace_entries", "start": 2,
            "entries": [
                {"recording_id": a, "chapter": f.anchors[0]},
                {"recording_id": b, "chapter": f.anchors[1]},
                {"recording_id": a, "chapter": f.anchors[2]}
            ]
        }),
    );
    let receipt = player.submit(&f.actor, accepted.clone()).await.unwrap();
    let before_player = serde_json::to_value(player.inspect(&f.actor).await.unwrap()).unwrap();
    let before_queue =
        serde_json::to_value(player.queue(&f.actor, lease, 1, 0, 10).await.unwrap()).unwrap();
    let anchor = |book, entry| json!({"audiobook_id": book, "entry_id": entry, "recording_id": a});
    let invalid = [
        ("foreign book", anchor(foreign, foreign_entry)),
        ("missing book", anchor(Uuid::new_v4(), Uuid::new_v4())),
        ("ungrouped playlist", anchor(ungrouped, ungrouped_entry)),
        ("deleted book", anchor(deleted, deleted_entry)),
        ("removed chapter", anchor(f.book, removed_entry)),
        ("missing chapter", anchor(f.book, Uuid::new_v4())),
        (
            "chapter belonging to another book",
            anchor(f.book, foreign_entry),
        ),
        ("chapter recording mismatch", anchor(f.book, f.chapters[1])),
    ];
    let rejected_id = Uuid::new_v4();
    for kind in ["replace_entries", "append_entries"] {
        for (case, chapter) in &invalid {
            let mut action = json!({
                "kind": kind,
                "entries": [
                    {"recording_id": a, "chapter": f.anchors[2]},
                    {"recording_id": a, "chapter": chapter}
                ]
            });
            if kind == "replace_entries" {
                action["start"] = json!(0);
            }
            assert_eq!(
                player
                    .submit(&f.actor, command(rejected_id, lease, 2, 1, action))
                    .await,
                Err(PlayerError::NotFound),
                "{kind} must atomically reject {case} even when both recordings are granted"
            );
            assert_eq!(
                player.submit(&f.actor, accepted.clone()).await.unwrap(),
                receipt
            );
            assert_eq!(
                serde_json::to_value(player.inspect(&f.actor).await.unwrap()).unwrap(),
                before_player
            );
            assert_eq!(
                serde_json::to_value(player.queue(&f.actor, lease, 1, 0, 10).await.unwrap())
                    .unwrap(),
                before_queue
            );
            assert_eq!(
                private_state(&f, foreign, ungrouped, deleted).await,
                before_private
            );
            assert_eq!(
                std::fs::read(f.source.music.join("track.wav")).unwrap(),
                f.source.original
            );
            assert_eq!(std::fs::read(&f.second_path).unwrap(), f.second_original);
        }
    }
    let corrected = command(
        rejected_id,
        lease,
        2,
        1,
        json!({
            "kind": "append_entries", "entries": [{"recording_id": b, "chapter": f.anchors[1]}]
        }),
    );
    let corrected_receipt = player.submit(&f.actor, corrected.clone()).await.unwrap();
    assert_eq!(corrected_receipt.revision, 2);
    assert_eq!(
        player.submit(&f.actor, corrected).await.unwrap(),
        corrected_receipt
    );
    let legacy = command(
        Uuid::new_v4(),
        lease,
        3,
        2,
        json!({"kind": "append", "recordings": [a]}),
    );
    player.submit(&f.actor, legacy).await.unwrap();
    let queue =
        serde_json::to_value(player.queue(&f.actor, lease, 3, 0, 10).await.unwrap()).unwrap();
    assert_eq!(queue["total"], 5);
    assert_eq!(queue["entries"][3]["chapter"], f.anchors[1]);
    assert!(queue["entries"][4].get("chapter").is_none());
    assert_eq!(
        &queue["entries"].as_array().unwrap()[..3],
        &before_queue["entries"].as_array().unwrap()[..3]
    );
    owner.close().await.unwrap();
    assert_eq!(
        private_state(&f, foreign, ungrouped, deleted).await,
        before_private
    );
}

fn command(id: Uuid, lease: Uuid, sequence: u64, revision: u64, action: Value) -> PlayerCommand {
    serde_json::from_value(json!({
        "id": id, "lease": lease, "sequence": sequence, "revision": revision, "action": action
    }))
    .unwrap()
}

async fn private_state(
    f: &audiobook::Library,
    foreign: Uuid,
    ungrouped: Uuid,
    deleted: Uuid,
) -> Value {
    let services = &f.source.services;
    for (actor, book) in [
        (&f.actor, foreign),
        (&f.actor, ungrouped),
        (&f.actor, deleted),
    ] {
        assert!(matches!(
            services
                .collection
                .audiobook(actor, &book.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    for actor in [&f.actor, &f.source.actor] {
        for recording in f.recordings {
            assert!(matches!(
                services
                    .downloads
                    .status(actor, &recording.to_string())
                    .await,
                Err(Error::NotFound)
            ));
        }
    }
    json!({
        "owner": {
            "book": services.collection.audiobook(&f.actor, &f.book.to_string()).await.unwrap(),
            "ungrouped": services.collection.playlist(&f.actor, &ungrouped.to_string()).await.unwrap(),
            "playlists": services.collection.playlists(&f.actor).await.unwrap(),
            "activity": services.collection.activity(&f.actor).await.unwrap(),
            "progress": services.collection.saved_playback(&f.actor, &f.recordings).await.unwrap(),
            "events": services.collection.events(&f.actor, 0).await.unwrap()
        },
        "foreign": {
            "book": services.collection.audiobook(&f.source.actor, &foreign.to_string()).await.unwrap(),
            "playlists": services.collection.playlists(&f.source.actor).await.unwrap(),
            "activity": services.collection.activity(&f.source.actor).await.unwrap(),
            "progress": services.collection.saved_playback(&f.source.actor, &f.recordings).await.unwrap(),
            "events": services.collection.events(&f.source.actor, 0).await.unwrap()
        }
    })
}
