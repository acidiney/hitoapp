#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{PlayerAction, PlayerSnapshot};
use std::time::Duration;

fn repeat() -> PlayerAction {
    serde_json::from_value(serde_json::json!({ "kind": "repeat" })).unwrap()
}

fn mode(state: &PlayerSnapshot) -> String {
    serde_json::to_value(state).unwrap()["repeat"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn repeat_one_restarts_only_natural_completion_and_receipt_replay_cannot_cycle_twice() {
    let mut f = fixture::Navigation::start(2, 0).await;
    let cycle = f.request(repeat());
    let receipt = f.submit(cycle.clone()).await;
    assert_eq!(
        f.player.submit(&f.source.actor, cycle).await.unwrap(),
        receipt
    );
    assert_eq!(mode(&f.state.borrow()), "all");
    f.send(repeat()).await;
    assert_eq!(mode(&f.state.borrow()), "one");
    assert_eq!(f.state.borrow().selection, 1);
    let entry = f.entries[0].id;
    f.send(PlayerAction::Resume {}).await;
    f.playing(entry).await;
    f.send(PlayerAction::Seek { position_ms: 9900 }).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|state| {
            state.selection == 2
                && state.current.is_some_and(|current| current.id == entry)
                && state.playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position
            .wait_for(|position| position.selection == 2 && position.position_ms >= 100),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(f.position.borrow().position_ms < 2000);
    assert!(f.state.borrow().desired_playing);
    f.send(PlayerAction::Pause {}).await;
    f.send(PlayerAction::Next {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[1].id);
    assert!(!f.state.borrow().desired_playing);
    f.send(repeat()).await;
    assert_eq!(mode(&f.state.borrow()), "off");
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn repeat_all_wraps_natural_and_requested_advancement_without_reordering_occurrences() {
    let mut f = fixture::Navigation::start(2, 1).await;
    f.send(repeat()).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[1].id).await;
    f.send(PlayerAction::Seek { position_ms: 9900 }).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|state| {
            state.selection == 2
                && state
                    .current
                    .is_some_and(|current| current.id == f.entries[0].id)
                && state.playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::Pause {}).await;
    f.send(PlayerAction::Select {
        entry: f.entries[1].id,
    })
    .await;
    f.send(PlayerAction::Next {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[0].id);
    assert!(!f.state.borrow().desired_playing);
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
        f.entries.iter().map(|entry| entry.id).collect::<Vec<_>>()
    );
    f.owner.close().await.unwrap();
}
