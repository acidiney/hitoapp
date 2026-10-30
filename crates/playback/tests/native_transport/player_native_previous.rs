#![cfg(feature = "device")]
#[path = "../fixtures/navigation.rs"]
mod fixture;
use hito_core::test_support::support;
use hito_playback::{PlayerAction, PlayerPhase};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn previous_follows_played_occurrences_instead_of_canonical_neighbors_and_replays_once() {
    let mut f = fixture::Navigation::start(3, 0).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Select {
        entry: f.entries[2].id,
    })
    .await;
    f.playing(f.entries[2].id).await;
    let previous = f.request(PlayerAction::Previous {});
    let receipt = f
        .player
        .submit(&f.source.actor, previous.clone())
        .await
        .unwrap();
    assert_eq!(
        f.player.submit(&f.source.actor, previous).await.unwrap(),
        receipt
    );
    let state = f
        .player
        .inspect(&f.source.actor)
        .await
        .unwrap()
        .state
        .unwrap();
    assert_eq!(state.current.unwrap().id, f.entries[0].id);
    assert_eq!(state.selection, 3);
    assert_eq!(state.revision, 3);
    assert!(state.desired_playing);
    let queue = f
        .player
        .queue(&f.source.actor, f.lease, 3, 0, 3)
        .await
        .unwrap();
    assert_eq!(
        queue
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        f.entries.iter().map(|entry| entry.id).collect::<Vec<_>>()
    );
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn previous_restarts_after_three_seconds_and_preserves_pause_and_the_queue_revision() {
    let mut f = fixture::Navigation::start(3, 0).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Select {
        entry: f.entries[2].id,
    })
    .await;
    f.playing(f.entries[2].id).await;
    f.send(PlayerAction::Pause {}).await;
    let seek = f.send(PlayerAction::Seek { position_ms: 4000 }).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.seek_id == Some(seek.id)),
    )
    .await
    .unwrap()
    .unwrap();
    let previous = f.send(PlayerAction::Previous {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[2].id);
    assert_eq!(previous.revision, 2);
    assert_eq!(f.state.borrow().selection, 2);
    assert!(!f.state.borrow().desired_playing);
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.seek_id == Some(previous.id)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(f.position.borrow().position_ms, 0);
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
async fn moved_occurrences_keep_history_and_removed_occurrences_are_skipped_while_paused() {
    let mut f = fixture::Navigation::start(4, 0).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    for index in [1, 3] {
        f.send(PlayerAction::Select {
            entry: f.entries[index].id,
        })
        .await;
        f.playing(f.entries[index].id).await;
    }
    f.send(PlayerAction::Pause {}).await;
    f.send(PlayerAction::Move {
        entry: f.entries[0].id,
        before: Some(f.entries[3].id),
    })
    .await;
    f.send(PlayerAction::Remove {
        entry: f.entries[1].id,
    })
    .await;
    f.send(PlayerAction::Previous {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[0].id);
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
