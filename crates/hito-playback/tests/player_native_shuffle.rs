#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::PlayerAction;
use std::collections::HashSet;

fn shuffle() -> PlayerAction {
    serde_json::from_value(serde_json::json!({ "kind": "shuffle" })).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn shuffle_visits_each_occurrence_once_keeps_canonical_order_and_replays_without_toggling() {
    let mut f = fixture::Navigation::start(5, 3).await;
    let toggle = f.request(shuffle());
    let receipt = f.submit(toggle.clone()).await;
    assert_eq!(
        f.player.submit(&f.source.actor, toggle).await.unwrap(),
        receipt
    );
    assert_eq!(
        serde_json::to_value(&*f.state.borrow()).unwrap()["shuffle"],
        true
    );
    assert_eq!(f.state.borrow().selection, 1);
    assert_eq!(f.state.borrow().revision, 1);
    let mut visited = HashSet::from([f.entries[3].id]);
    for _ in 1..5 {
        f.send(PlayerAction::Next {}).await;
        let entry = f.state.borrow().current.unwrap().id;
        assert!(
            visited.insert(entry),
            "Shuffle repeated an occurrence before completing its cycle"
        );
        assert!(!f.state.borrow().desired_playing);
    }
    assert_eq!(visited, f.entries.iter().map(|entry| entry.id).collect());
    f.send(PlayerAction::Next {}).await;
    assert!(f.state.borrow().current.is_none());
    let revision = f.state.borrow().revision;
    let page = f
        .player
        .queue(&f.source.actor, f.lease, revision, 0, 5)
        .await
        .unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        f.entries.iter().map(|entry| entry.id).collect::<Vec<_>>()
    );
    f.send(shuffle()).await;
    assert_eq!(
        serde_json::to_value(&*f.state.borrow()).unwrap()["shuffle"],
        false
    );
    f.send(PlayerAction::Resume {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[0].id);
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn append_move_and_current_removal_preserve_live_shuffle_successors() {
    let mut f = fixture::Navigation::start(2, 0).await;
    f.send(shuffle()).await;
    f.send(PlayerAction::Append {
        recordings: vec![f.entries[0].recording.id.parse().unwrap()],
    })
    .await;
    let revision = f.state.borrow().revision;
    let appended = f
        .player
        .queue(&f.source.actor, f.lease, revision, 2, 1)
        .await
        .unwrap()
        .entries
        .remove(0)
        .id;
    f.send(PlayerAction::Move {
        entry: f.entries[1].id,
        before: Some(f.entries[0].id),
    })
    .await;
    f.send(PlayerAction::Next {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[1].id);
    f.send(PlayerAction::Remove {
        entry: f.entries[1].id,
    })
    .await;
    assert_eq!(f.state.borrow().current.unwrap().id, appended);
    assert!(!f.state.borrow().desired_playing);
    f.send(PlayerAction::Next {}).await;
    assert!(f.state.borrow().current.is_none());
    let revision = f.state.borrow().revision;
    let page = f
        .player
        .queue(&f.source.actor, f.lease, revision, 0, 2)
        .await
        .unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![f.entries[0].id, appended]
    );
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn replace_and_clear_append_rebuild_shuffle_without_old_occurrences() {
    let mut f = fixture::Navigation::start(3, 0).await;
    f.send(shuffle()).await;
    let recording = f.entries[0].recording.id.parse().unwrap();
    for replace in [true, false] {
        if replace {
            f.send(PlayerAction::Replace {
                recordings: vec![recording; 3],
                start: 2,
            })
            .await;
        } else {
            f.send(PlayerAction::Clear {}).await;
            f.send(PlayerAction::Append {
                recordings: vec![recording; 3],
            })
            .await;
            f.send(PlayerAction::Resume {}).await;
            f.send(PlayerAction::Pause {}).await;
        }
        let revision = f.state.borrow().revision;
        let page = f
            .player
            .queue(&f.source.actor, f.lease, revision, 0, 3)
            .await
            .unwrap();
        assert!(
            page.entries
                .iter()
                .all(|entry| !f.entries.iter().any(|old| old.id == entry.id))
        );
        let mut visited = HashSet::from([f.state.borrow().current.unwrap().id]);
        for _ in 1..3 {
            f.send(PlayerAction::Next {}).await;
            assert!(visited.insert(f.state.borrow().current.unwrap().id));
        }
        assert_eq!(visited, page.entries.iter().map(|entry| entry.id).collect());
        f.send(PlayerAction::Next {}).await;
        assert!(f.state.borrow().current.is_none());
        assert_eq!(
            serde_json::to_value(&*f.state.borrow()).unwrap()["shuffle"],
            true
        );
    }
    f.owner.close().await.unwrap();
}
