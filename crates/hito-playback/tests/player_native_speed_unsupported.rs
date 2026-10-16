#![cfg(feature = "device")]
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "fixtures/recovery.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn unsupported_tempo_is_visible_and_unit_speed_can_retry_the_same_occurrence() {
    let f = fixture::Recovery::start_with_wave(384_000, 768_000).await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    let mut state = player.subscribe(&f.actor, lease).await.unwrap();
    let mut position = player.subscribe_position(&f.actor, lease).await.unwrap();
    for (sequence, revision, action) in [
        (1, 0, PlayerAction::Speed { rate: 500 }),
        (
            2,
            0,
            PlayerAction::Replace {
                recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                start: 0,
            },
        ),
    ] {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease,
                    sequence,
                    revision,
                    action,
                },
            )
            .await
            .unwrap();
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        state.wait_for(|s| s.phase == PlayerPhase::Failed),
    )
    .await
    .unwrap()
    .unwrap();
    let failed = state.borrow().clone();
    assert_eq!(failed.error, Some(PlayerError::Playback));
    assert_eq!(failed.last_failure.unwrap().error, PlayerError::Playback);
    assert!(!failed.playing);
    assert!(!failed.desired_playing);
    assert_eq!(position.borrow().position_ms, 0);
    capture.after(capture.mark(), 0.2);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert!(
        f.services
            .collection
            .activity(&f.actor)
            .await
            .unwrap()
            .history
            .is_empty()
    );
    for (sequence, action) in [
        (3, PlayerAction::Speed { rate: 1000 }),
        (4, PlayerAction::Resume {}),
    ] {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease,
                    sequence,
                    revision: 1,
                    action,
                },
            )
            .await
            .unwrap();
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        position.wait_for(|p| p.position_ms >= 200),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(state.borrow().current, failed.current);
    assert_eq!(state.borrow().selection, failed.selection);
    assert_eq!(state.borrow().speed, 1000);
    assert!(capture::energy(&capture.recent(0.1)) > 0.001);
    owner.close().await.unwrap();
}
