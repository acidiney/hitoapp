#![cfg(feature = "device")]
use hito_audio::test_support as audio_support;
use hito_audio::test_support::capture;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

async fn tone_library() -> audio_fixture::AudioLibrary {
    let mut f = audio_fixture::AudioLibrary::start().await;
    let mut wave = audio_support::wave(48_000, 1, 48_000 * 10, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
    });
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut wave, &mut bytes).unwrap();
    f.original = bytes;
    f.ready().await;
    f
}

async fn play(f: &audio_fixture::AudioLibrary, player: &Player) -> hito_playback::PlayerAttachment {
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
        (2, 1, PlayerAction::Resume {}),
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
    attached
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_connection_reset_mid_track_recovers_from_a_fresh_provider_request_past_the_break() {
    let f = tone_library().await;
    let reset = f.server.hold_broken_tail(65_536).await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.hito.clone());
    let attached = play(&f, &player).await;
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), reset.entered())
        .await
        .unwrap();
    f.server.clear_audio_tail_fault().await;
    tokio::time::timeout(
        Duration::from_secs(10),
        position.wait_for(|state| state.position_ms >= 3_000),
    )
    .await
    .expect("playback must continue past the reset")
    .unwrap();
    assert_eq!(state.borrow_and_update().phase, PlayerPhase::Ready);
    assert!(state.borrow().playing);
    assert_eq!(state.borrow().selection, 1);
    let requests = f.server.requests("v1/audio").await.len();
    assert!(
        (2..=3).contains(&requests),
        "recovery must use a bounded number of fresh requests, saw {requests}"
    );
    capture.after(capture.mark(), 0.1);
    assert!(capture::amplitude(&capture.recent(0.05), 440.) > 0.2);
    owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_slow_first_byte_delays_the_start_without_failing_the_track() {
    let f = tone_library().await;
    f.server.audio_first_byte_delay(Duration::from_secs(2));
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.hito.clone());
    let started = std::time::Instant::now();
    let attached = play(&f, &player).await;
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(8),
        position.wait_for(|state| state.position_ms >= 500),
    )
    .await
    .expect("a slow provider must still start the track")
    .unwrap();
    assert!(started.elapsed() >= Duration::from_secs(2));
    assert_eq!(f.server.requests("v1/audio").await.len(), 1);
    capture.after(capture.mark(), 0.1);
    assert!(capture::amplitude(&capture.recent(0.05), 440.) > 0.2);
    owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_seek_whose_link_expired_recovers_through_a_refresh_at_the_requested_position() {
    let f = tone_library().await;
    f.server.range_chunks(1_000_000).await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.hito.clone());
    let attached = play(&f, &player).await;
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        position.wait_for(|state| state.position_ms >= 500),
    )
    .await
    .unwrap()
    .unwrap();
    f.server.audio_statuses(vec![axum::http::StatusCode::GONE]);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 1,
                action: PlayerAction::Seek { position_ms: 6_000 },
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(8),
        position.wait_for(|state| state.position_ms >= 6_000),
    )
    .await
    .expect("the seek must recover at the requested position")
    .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        state.wait_for(|state| state.playing && state.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        f.server.requests("v1/audio").await.len() >= 3,
        "the expired seek request and a recovery request must both reach the provider"
    );
    assert!(
        f.server.requests("v1/sources").await.len() <= 3,
        "recovery must not loop on source refreshes"
    );
    capture.after(capture.mark(), 0.1);
    assert!(capture::amplitude(&capture.recent(0.05), 440.) > 0.2);
    owner.close().await.unwrap();
}
