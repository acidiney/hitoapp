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
async fn a_failed_zero_seek_recovers_at_zero_instead_of_the_older_observed_position() {
    let mut f = audio_fixture::AudioLibrary::start().await;
    let mut wave = audio_support::wave(48_000, 1, 48_000 * 10, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
    });
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut wave, &mut bytes).unwrap();
    f.original = bytes;
    f.ready().await;
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
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 1,
                action: PlayerAction::Pause,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            !state.playing && !state.desired_playing && state.phase == PlayerPhase::Ready
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let gate = f.server.hold_range(44..45).await;
    let refresh = f.server.hold("v1/sources").await;
    let mut headers = audio_fixture::headers();
    headers.insert("content-type", "application/json".parse().unwrap());
    f.server
        .audio(axum::http::StatusCode::OK, headers, f.original.clone())
        .await;
    let zero = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 4,
                revision: 1,
                action: PlayerAction::Seek { position_ms: 0 },
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    gate.open();
    tokio::time::timeout(Duration::from_secs(3), refresh.entered())
        .await
        .unwrap();
    assert_ne!(position.borrow().seek_id, Some(zero.id));
    f.ready().await;
    refresh.release();
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
            !state.seeking && !state.playing && state.phase == PlayerPhase::Ready
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(position.borrow().position_ms, 0);
    assert_eq!(state.borrow().selection, 1);
    assert_eq!(state.borrow().revision, 1);
    assert_eq!(f.server.requests("v1/sources").await.len(), 2);
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    owner.close().await.unwrap();
}
