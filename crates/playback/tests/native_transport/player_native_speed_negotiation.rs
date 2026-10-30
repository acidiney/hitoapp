#![cfg(feature = "device")]
use hito_audio::test_support::capture;
use hito_core::test_support::support;
#[path = "../fixtures/recovery.rs"]
mod fixture;
use hito_playback::{
    Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase, PlayerSpeedSupport,
};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_high_rate_source_reports_its_speed_range_and_plays_at_the_chosen_rate() {
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
        (3, 1, PlayerAction::Resume {}),
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
        state.wait_for(|s| s.phase == PlayerPhase::Ready && s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    let ready = state.borrow().clone();
    assert_eq!(ready.error, None);
    assert!(ready.last_failure.is_none());
    assert_eq!(ready.speed, 500);
    assert_eq!(ready.effective_speed, 500);
    assert_eq!(
        ready.speed_support,
        Some(PlayerSpeedSupport {
            min: 500,
            max: 3000
        })
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        position.wait_for(|p| p.position_ms >= 100),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(capture::energy(&capture.recent(0.1)) > 0.001);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 4,
                revision: 1,
                action: PlayerAction::Speed { rate: 2000 },
            },
        )
        .await
        .unwrap();
    assert_eq!(state.borrow().effective_speed, 2000);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(600);
    while tokio::time::Instant::now() < deadline {
        let seen = state.borrow_and_update().clone();
        assert_eq!(
            seen.effective_speed, seen.speed,
            "a capable source never presents a different speed"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let faster = state.borrow().clone();
    assert!(faster.playing);
    assert_eq!(faster.phase, PlayerPhase::Ready);
    assert_eq!(faster.error, None);
    assert_ne!(faster.error, Some(PlayerError::Playback));
    assert_eq!(faster.speed, 2000);
    assert!(capture::energy(&capture.recent(0.1)) > 0.001);
    owner.close().await.unwrap();
}
