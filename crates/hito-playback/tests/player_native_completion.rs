#![cfg(feature = "device")]
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn natural_completion_plays_the_next_occurrence_and_finishes_the_queue() {
    let f = fixture::LocalPlayback::start().await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let queued = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap(); 2],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let first = player
        .inspect(&f.actor)
        .await
        .unwrap()
        .state
        .unwrap()
        .current
        .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: queued.revision,
                action: PlayerAction::Resume,
            },
        )
        .await
        .unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    let second = *tokio::time::timeout(
        Duration::from_secs(4),
        position.wait_for(|state| state.selection == 2 && state.position_ms > 100),
    )
    .await
    .unwrap()
    .unwrap();
    assert_ne!(second.entry, Some(first.id));
    let current = player
        .inspect(&f.actor)
        .await
        .unwrap()
        .state
        .unwrap()
        .current
        .unwrap();
    assert_eq!(current.id, second.entry.unwrap());
    assert_eq!(current.recording_id, first.recording_id);
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.current.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    let finished = state.borrow().clone();
    assert_eq!(finished.queue_len, 2);
    assert_eq!(finished.revision, queued.revision + 2);
    assert_eq!(finished.phase, PlayerPhase::Idle);
    assert!(!finished.desired_playing);
    assert!(!finished.playing);
    owner.close().await.unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
}
