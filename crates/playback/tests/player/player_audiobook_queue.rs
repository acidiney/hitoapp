#[path = "../fixtures/audiobook_queue.rs"]
mod audiobook;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;

use hito_core::{Actor, Error};
use hito_playback::{Player, PlayerCommand, PlayerReceipt};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn audiobook_queue_keeps_chapter_occurrences_across_retries_edits_and_reattachment() {
    let audiobook::Library {
        source: f,
        actor,
        recordings: [a, b],
        book,
        chapters,
        anchors,
        second_path,
        second_original,
    } = audiobook::Library::start().await;
    let before_book = serde_json::to_value(
        f.services
            .collection
            .audiobook(&actor, &book.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(before_book.get("continuation").is_none());
    let before_events =
        serde_json::to_value(f.services.collection.events(&actor, 0).await.unwrap()).unwrap();
    let (player, owner) = Player::start(f.services.clone());
    let lease = player
        .claim(&actor, player.inspect(&actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    submit(
        &player,
        &actor,
        lease,
        1,
        0,
        json!({
            "kind": "replace", "recordings": [a], "start": 0
        }),
    )
    .await;
    let legacy = player.queue(&actor, lease, 1, 0, 1).await.unwrap();
    assert!(
        serde_json::to_value(&legacy.entries[0])
            .unwrap()
            .get("chapter")
            .is_none()
    );
    let replacement = serde_json::to_string(&json!({
        "id": Uuid::new_v4(), "lease": lease, "sequence": 2, "revision": 1,
        "action": {
            "kind": "replace_entries", "start": 2,
            "entries": [
                {"recording_id": a, "chapter": anchors[0]},
                {"recording_id": b, "chapter": anchors[1]},
                {"recording_id": a, "chapter": anchors[2]}
            ]
        }
    }))
    .unwrap();
    let receipt = player
        .submit(
            &actor,
            serde_json::from_str::<PlayerCommand>(&replacement)
                .expect("The player must accept chapter-bearing queue occurrences"),
        )
        .await
        .unwrap();
    let first_page = player
        .queue(&actor, lease, receipt.revision, 0, 1)
        .await
        .unwrap();
    let second_page = player
        .queue(&actor, lease, receipt.revision, 1, 2)
        .await
        .unwrap();
    assert_eq!(
        (
            first_page.total,
            first_page.offset,
            first_page.entries.len()
        ),
        (3, 0, 1)
    );
    assert_eq!(
        (
            second_page.total,
            second_page.offset,
            second_page.entries.len()
        ),
        (3, 1, 2)
    );
    let entries = first_page
        .entries
        .into_iter()
        .chain(second_page.entries)
        .collect::<Vec<_>>();
    let ids = entries.iter().map(|entry| entry.id).collect::<Vec<_>>();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    for (index, entry) in entries.iter().enumerate() {
        let encoded = serde_json::to_value(entry).unwrap();
        assert_eq!(encoded["chapter"], anchors[index]);
        assert_eq!(encoded["recording"]["id"], anchors[index]["recording_id"]);
        assert!(!chapters.contains(&entry.id));
    }
    let selected = serde_json::to_value(player.inspect(&actor).await.unwrap()).unwrap();
    assert_eq!(selected["state"]["current"]["id"], json!(ids[2]));
    assert_eq!(selected["state"]["current"]["chapter"], anchors[2]);
    let unchanged =
        serde_json::to_value(player.queue(&actor, lease, 2, 0, 10).await.unwrap()).unwrap();
    assert_eq!(
        player
            .submit(&actor, serde_json::from_str(&replacement).unwrap())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        serde_json::to_value(player.inspect(&actor).await.unwrap()).unwrap(),
        selected
    );
    assert_eq!(
        serde_json::to_value(player.queue(&actor, lease, 2, 0, 10).await.unwrap()).unwrap(),
        unchanged
    );
    submit(
        &player,
        &actor,
        lease,
        3,
        2,
        json!({
            "kind": "append_entries", "entries": [{"recording_id": b, "chapter": anchors[1]}]
        }),
    )
    .await;
    let appended = player
        .queue(&actor, lease, 3, 3, 1)
        .await
        .unwrap()
        .entries
        .remove(0);
    assert!(!ids.contains(&appended.id));
    assert_eq!(
        serde_json::to_value(&appended).unwrap()["chapter"],
        anchors[1]
    );
    submit(
        &player,
        &actor,
        lease,
        4,
        3,
        json!({
            "kind": "move", "entry": ids[0], "before": null
        }),
    )
    .await;
    submit(
        &player,
        &actor,
        lease,
        5,
        4,
        json!({
            "kind": "remove", "entry": ids[0]
        }),
    )
    .await;
    let after = player.inspect(&actor).await.unwrap();
    let state = serde_json::to_value(after.state.as_ref().unwrap()).unwrap();
    assert_eq!(state["current"], selected["state"]["current"]);
    assert_eq!(state["selection"], selected["state"]["selection"]);
    let queue = serde_json::to_value(player.queue(&actor, lease, 5, 0, 10).await.unwrap()).unwrap();
    assert_eq!(queue["total"], 3);
    assert_eq!(queue["entries"][0]["id"], json!(ids[1]));
    assert_eq!(queue["entries"][1]["id"], json!(ids[2]));
    assert_eq!(queue["entries"][1]["chapter"], anchors[2]);
    assert_eq!(queue["entries"][2]["id"], json!(appended.id));
    let reattached = player.claim(&actor, after.epoch).await.unwrap();
    assert_ne!(reattached.lease, lease);
    assert_eq!(serde_json::to_value(&reattached.state).unwrap(), state);
    assert_eq!(
        serde_json::to_value(
            player
                .queue(&actor, reattached.lease, 5, 0, 10)
                .await
                .unwrap()
        )
        .unwrap(),
        queue
    );
    submit(
        &player,
        &actor,
        reattached.lease,
        1,
        5,
        json!({"kind": "next"}),
    )
    .await;
    let next = serde_json::to_value(player.inspect(&actor).await.unwrap()).unwrap();
    assert_eq!(next["state"]["current"]["id"], json!(appended.id));
    assert_eq!(next["state"]["current"]["chapter"], anchors[1]);
    owner.close().await.unwrap();
    assert_eq!(
        serde_json::to_value(
            f.services
                .collection
                .audiobook(&actor, &book.to_string())
                .await
                .unwrap(),
        )
        .unwrap(),
        before_book,
        "Queue selection must not persist an unobserved chapter"
    );
    assert_eq!(
        serde_json::to_value(f.services.collection.events(&actor, 0).await.unwrap(),).unwrap(),
        before_events,
        "Queue acceptance must not create listening progress events"
    );
    assert!(
        f.services
            .collection
            .saved_playback(&actor, &[a, b])
            .await
            .unwrap()
            .is_empty()
    );
    for recording in [a, b] {
        assert!(matches!(
            f.services
                .downloads
                .status(&actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(
        std::fs::read(f.music.join("track.wav")).unwrap(),
        f.original
    );
    assert_eq!(std::fs::read(second_path).unwrap(), second_original);
}

async fn submit(
    player: &Player,
    actor: &Actor,
    lease: Uuid,
    sequence: u64,
    revision: u64,
    action: Value,
) -> PlayerReceipt {
    let command = serde_json::from_value::<PlayerCommand>(json!({
        "id": Uuid::new_v4(), "lease": lease, "sequence": sequence,
        "revision": revision, "action": action
    }))
    .unwrap();
    player.submit(actor, command).await.unwrap()
}
