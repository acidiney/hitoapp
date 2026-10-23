#![cfg(feature = "device")]
use hito_audio::test_support as audio_support;
use hito_audio::test_support::capture;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::test_support::support;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn next_cancels_held_provider_startup_without_waiting_for_its_reply_or_leaking_admission() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    let mut audio = audio_support::wave(48_000, 1, 48_000 * 4, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
    });
    std::io::copy(
        &mut audio,
        &mut std::fs::File::create(f.music.join("next.wav")).unwrap(),
    )
    .unwrap();
    let library = f
        .hito
        .catalog
        .libraries(&f.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|library| !library.personal)
        .unwrap()
        .id;
    support::scan(&f.hito, &f.actor, &library).await;
    let next = f
        .hito
        .catalog
        .recordings(
            &f.actor,
            hito_core::Search {
                q: Some("next".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
        .remove(0);
    let capture = capture::Capture::open();
    let gate = f.server.hold("audio-tail").await;
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
                    recordings: vec![
                        Uuid::parse_str(&f.source.recording_id).unwrap(),
                        Uuid::parse_str(&next.id).unwrap(),
                    ],
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
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let start = Instant::now();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: playing.revision,
                action: PlayerAction::Next {},
            },
        )
        .await
        .unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "admission waited for the old source"
    );
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|state| state.selection == 2 && state.position_ms > 100),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.1);
    assert!(capture::amplitude(&capture.recent(0.1), 440.) > 0.2);
    let first = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    let second = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    drop((first, second));
    gate.release();
    capture.after(capture.mark(), 0.1);
    assert_eq!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .unwrap()
            .recording_id
            .to_string(),
        next.id
    );
    assert!(capture::amplitude(&capture.recent(0.1), 440.) > 0.2);
    owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn close_joins_held_provider_startup_without_waiting_for_the_provider_response() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-tail").await;
    let capture = capture::Capture::open();
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
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), owner.close())
        .await
        .unwrap()
        .unwrap();
    assert!(state.has_changed().is_err());
    let first = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    let second = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    drop((first, second));
    gate.release();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
}
