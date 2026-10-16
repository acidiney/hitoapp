#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "../../hito-audio/tests/support/mod.rs"]
#[allow(dead_code)]
mod audio_support;
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pause_and_zero_seek_supersede_a_blocked_forward_seek_without_losing_the_new_target() {
    let f = audio_fixture::AudioLibrary::start().await;
    let mut wave = audio_support::wave(48_000, 1, 48_000 * 10, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
    });
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut wave, &mut bytes).unwrap();
    f.server
        .audio(axum::http::StatusCode::OK, audio_fixture::headers(), bytes)
        .await;
    f.server.range_chunks(1_000_000).await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.hito.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    for (sequence, revision, action) in [
        (
            1,
            0,
            PlayerAction::Replace {
                recordings: vec![Uuid::parse_str(&f.source.recording_id).unwrap()],
                start: 0,
            },
        ),
        (2, 1, PlayerAction::Resume),
    ] {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence,
                    revision,
                    action,
                },
            )
            .await
            .unwrap();
    }
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(4),
        position.wait_for(|position| position.position_ms >= 1500),
    )
    .await
    .unwrap()
    .unwrap();
    let gate = f.server.hold_range(600_000..800_000).await;
    let blocked = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 1,
                action: PlayerAction::Seek { position_ms: 7000 },
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    assert_ne!(position.borrow().seek_id, Some(blocked.id));
    assert!(state.borrow().seeking);
    let started = std::time::Instant::now();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 4,
                revision: 1,
                action: PlayerAction::Pause,
            },
        )
        .await
        .unwrap();
    let zero = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 5,
                revision: 1,
                action: PlayerAction::Seek { position_ms: 0 },
            },
        )
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.seek_id == Some(zero.id)),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready
                && !state.playing
                && !state.seeking
                && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(position.borrow().position_ms, 0);
    gate.open();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert_eq!(position.borrow().position_ms, 0);
    owner.close().await.unwrap();
}
