#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
use hito_core::test_support::support;
use hito_playback::{PlayerAction, PlayerPhase};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn previous_follows_audible_shuffle_history_and_next_reuses_its_successor_while_paused() {
    let mut f = fixture::Navigation::start(3, 2).await;
    f.send(PlayerAction::Shuffle {}).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[2].id).await;
    f.send(PlayerAction::Next {}).await;
    let next = f.state.borrow().current.unwrap().id;
    f.playing(next).await;
    f.send(PlayerAction::Pause {}).await;
    f.send(PlayerAction::Previous {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[2].id);
    f.send(PlayerAction::Next {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, next);
    assert!(!f.state.borrow().desired_playing);
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state
            .wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn natural_shuffle_completion_wraps_repeat_all_and_repeat_one_overrides_only_completion() {
    let mut f = fixture::Navigation::start(2, 1).await;
    f.send(PlayerAction::Shuffle {}).await;
    f.send(PlayerAction::Repeat {}).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[1].id).await;
    for (selection, index) in [(2, 0), (3, 1), (4, 1)] {
        if selection == 4 {
            f.send(PlayerAction::Repeat {}).await;
        }
        f.send(PlayerAction::Seek { position_ms: 9900 }).await;
        tokio::time::timeout(
            Duration::from_secs(3),
            f.state.wait_for(|s| {
                s.selection == selection
                    && s.current
                        .is_some_and(|entry| entry.id == f.entries[index].id)
                    && s.playing
            }),
        )
        .await
        .unwrap()
        .unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            f.position
                .wait_for(|p| p.selection == selection && p.position_ms >= 100),
        )
        .await
        .unwrap()
        .unwrap();
    }
    f.send(PlayerAction::Pause {}).await;
    f.send(PlayerAction::Next {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[0].id);
    assert!(!f.state.borrow().desired_playing);
    f.owner.close().await.unwrap();
}
