#![cfg(feature = "device")]
use hito_audio::test_support::capture;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn previous_retries_a_failed_paused_occurrence_without_starting_output() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.server.range_chunks(1_000_000).await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.hito.clone());
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.source.recording_id).unwrap()],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, lease).await.unwrap();
    let mut position = player.subscribe_position(&f.actor, lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|s| s.phase == PlayerPhase::Failed),
    )
    .await
    .unwrap()
    .unwrap();
    let selected = state.borrow().current;
    f.ready().await;
    let previous = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::Previous {},
            },
        )
        .await
        .unwrap();
    let applied = tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|p| p.seek_id == Some(previous.id)),
    )
    .await
    .is_ok();
    let requests = f.server.requests("v1/audio").await.len();
    assert!(
        applied,
        "state={:?}; position={:?}; audio_requests={requests}",
        *state.borrow(),
        *position.borrow()
    );
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(state.borrow().current, selected);
    assert_eq!(state.borrow().revision, 1);
    assert_eq!(state.borrow().selection, 1);
    assert!(!state.borrow().desired_playing);
    assert_eq!(position.borrow().position_ms, 0);
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    owner.close().await.unwrap();
}
