#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "../../hito-audio/tests/support/mod.rs"]
#[allow(dead_code)]
mod audio_support;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pausing_a_blocked_restore_retains_the_saved_position_across_another_reopen() {
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
    let (player, owner) = Player::native(f.hito.clone());
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
                    recordings: vec![Uuid::parse_str(&f.source.recording_id).unwrap()],
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
                action: PlayerAction::Resume,
            },
        )
        .await
        .unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        position.wait_for(|state| state.position_ms >= 2_000),
    )
    .await
    .unwrap()
    .unwrap();
    let refresh = f.server.hold("v1/sources").await;
    f.hito
        .plugins
        .clear_credential(&f.actor, &f.installation.id, 1)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), refresh.entered())
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let saved = position.borrow().position_ms;
    assert!(saved >= 2_000);
    let gate = f.server.hold_range(100_000..800_000).await;
    refresh.release();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: queued.revision,
                action: PlayerAction::Pause,
            },
        )
        .await
        .unwrap();
    assert!(
        position.borrow().position_ms >= saved,
        "an unconfigured restore published zero"
    );
    gate.open();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        position.borrow().position_ms >= saved,
        "pause lost the retained restore target"
    );
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 4,
                revision: queued.revision,
                action: PlayerAction::Resume,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|state| state.position_ms > saved + 100),
    )
    .await
    .unwrap()
    .unwrap();
    owner.close().await.unwrap();
}
