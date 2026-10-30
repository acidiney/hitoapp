#![cfg(feature = "device")]
use hito_audio::test_support::capture;
use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn authenticated_queue_commands_drive_actual_output_and_join_it_on_close() {
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
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: queued.revision,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|state| state.position_ms > 100),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.1)) > 0.00001);
    owner.close().await.unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert!(position.has_changed().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pause_preserves_position_and_reattachment_keeps_the_same_audio_selection() {
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
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let playing = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: queued.revision,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|state| state.position_ms >= 100),
    )
    .await
    .unwrap()
    .unwrap();
    let paused = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: playing.revision,
                action: PlayerAction::Pause {},
            },
        )
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        state.wait_for(|state| !state.playing && state.phase == hito_playback::PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    let stopped_at = position.borrow().position_ms;
    capture.after(capture.mark(), 0.1);
    assert!(
        position.borrow().position_ms.abs_diff(stopped_at) < 10,
        "paused cursor moved from {stopped_at} to {}; phase={:?}",
        position.borrow().position_ms,
        state.borrow().phase
    );
    let renewed = player.claim(&f.actor, attached.epoch).await.unwrap();
    assert_eq!(renewed.state.current, state.borrow().current);
    assert_eq!(renewed.state.selection, state.borrow().selection);
    assert!(!renewed.state.desired_playing);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: renewed.lease,
                sequence: 1,
                revision: paused.revision,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    let mut renewed_position = player
        .subscribe_position(&f.actor, renewed.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        renewed_position.wait_for(|state| state.position_ms > stopped_at + 80),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    owner.close().await.unwrap();
}
