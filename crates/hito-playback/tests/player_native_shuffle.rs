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
