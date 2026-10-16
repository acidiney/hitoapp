#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{PlayerAction, PlayerPhase};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn speed_is_applied_before_playback_and_saved_with_source_time_progress() {
    let mut f = fixture::Navigation::start(1, 0).await;
    let command = serde_json::from_str::<PlayerAction>(r#"{"kind":"speed","rate":2000}"#)
        .expect("native speed is a public command");
    f.send(command).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Pause {}).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let activity = f
                .source
                .services
                .collection
                .activity(&f.source.actor)
                .await
                .unwrap();
            if activity.history.iter().any(|item| {
                item.id == f.source.recording && item.speed == 2. && item.position_ms > 100
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn paused_rate_changes_preserve_selection_seek_and_exact_receipts() {
    let mut f = fixture::Navigation::start(2, 0).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Pause {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state
            .wait_for(|state| state.phase == PlayerPhase::Ready && !state.playing),
    )
    .await
    .unwrap()
    .unwrap();
    let seek = f.send(PlayerAction::Seek { position_ms: 1500 }).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position
            .wait_for(|position| position.seek_id == Some(seek.id)),
    )
    .await
    .unwrap()
    .unwrap();
    let before = f.state.borrow().clone();
    let command = f.request(PlayerAction::Speed { rate: 1500 });
    let receipt = f.submit(command.clone()).await;
    assert_eq!(
        f.player.submit(&f.source.actor, command).await.unwrap(),
        receipt
    );
    let state = f
        .player
        .inspect(&f.source.actor)
        .await
        .unwrap()
        .state
        .unwrap();
    assert_eq!(state.speed, 1500);
    assert_eq!(state.selection, before.selection);
    assert_eq!(state.revision, before.revision);
    assert_eq!(state.current, before.current);
    assert!(!state.desired_playing);
    assert_eq!(f.position.borrow().position_ms, 1500);
    f.send(PlayerAction::Resume {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|position| position.position_ms >= 2100),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::Next {}).await;
    f.playing(f.entries[1].id).await;
    assert_eq!(f.state.borrow().speed, 1500);
    f.owner.close().await.unwrap();
}
