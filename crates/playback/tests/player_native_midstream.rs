#![cfg(feature = "device")]
use hito_audio::test_support::capture;
#[path = "fixtures/recovery.rs"]
mod fixture;
use hito_core::test_support::support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn withdrawing_the_playing_copy_recovers_another_source_near_the_observed_position() {
    let f = fixture::Recovery::start().await;
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
        position.wait_for(|state| state.position_ms >= 500),
    )
    .await
    .unwrap()
    .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let original = state.borrow().source_id.clone().unwrap();
    let before = position.borrow().position_ms;
    f.services
        .storage
        .remove_folder(&f.actor, &f.first.id, f.first.revision)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state
                .source_id
                .as_ref()
                .is_some_and(|source| source != &original)
                && state.playing
                && state.phase == PlayerPhase::Ready
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let recovered = position.borrow().position_ms;
    assert!(
        recovered >= before,
        "recovery restarted before the observed position"
    );
    assert!(
        recovered < before + 1_000,
        "recovery jumped away from the observed position"
    );
    assert_eq!(state.borrow().selection, 1);
    assert_eq!(state.borrow().revision, queued.revision);
    capture.after(capture.mark(), 0.1);
    assert!(capture::amplitude(&capture.recent(0.05), 440.) > 0.2);
    owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn withdrawing_a_paused_copy_recovers_without_starting_output() {
    let f = fixture::Recovery::start().await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    player
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
                revision: 1,
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
        position.wait_for(|state| state.position_ms >= 500),
    )
    .await
    .unwrap()
    .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 1,
                action: PlayerAction::Pause {},
            },
        )
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.phase == PlayerPhase::Ready && !state.playing),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.3);
    let original = state.borrow().source_id.clone().unwrap();
    let before = position.borrow().position_ms;
    f.services
        .storage
        .remove_folder(&f.actor, &f.first.id, f.first.revision)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state
                .source_id
                .as_ref()
                .is_some_and(|source| source != &original)
                && !state.playing
                && state.phase == PlayerPhase::Ready
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!state.borrow().desired_playing);
    assert!(position.borrow().position_ms >= before);
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    owner.close().await.unwrap();
}
