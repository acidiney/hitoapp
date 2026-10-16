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
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn next_cancels_refresh_before_publication_and_late_provider_data_cannot_replace_it() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.server
        .audio(
            axum::http::StatusCode::BAD_GATEWAY,
            Default::default(),
            Vec::new(),
        )
        .await;
    f.server
        .reply(
            "v1/sources",
            serde_json::json!({"items":[{"id":"late","label":"Original","ready":true}]}),
        )
        .await;
    let gate = f.server.hold("v1/sources").await;
    let mut wave = audio_support::wave(48_000, 1, 48_000 * 4, |frame, _| {
        ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
    });
    std::io::copy(
        &mut wave,
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
    let (player, owner) = Player::native(f.hito.clone());
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
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let started = Instant::now();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 1,
                action: PlayerAction::Next {},
            },
        )
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
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
    assert!(capture::amplitude(&capture.recent(0.05), 440.) > 0.2);
    tokio::time::timeout(Duration::from_secs(2), owner.close())
        .await
        .unwrap()
        .unwrap();
    gate.release();
    let sources = f
        .hito
        .catalog
        .sources(&f.actor, &f.source.recording_id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, f.source.id);
}
