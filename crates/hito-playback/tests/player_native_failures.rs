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
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn exhausted_cached_and_refreshed_sources_report_failure_and_continue_the_queue() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.server
        .audio(
            axum::http::StatusCode::BAD_GATEWAY,
            Default::default(),
            Vec::new(),
        )
        .await;
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
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(4),
        position.wait_for(|state| state.selection == 2 && state.position_ms > 100),
    )
    .await
    .unwrap()
    .unwrap();
    let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    let failure = state.last_failure.unwrap();
    assert_eq!(
        failure.entry.recording_id.to_string(),
        f.source.recording_id
    );
    assert_eq!(failure.error, hito_playback::PlayerError::Playback);
    assert_eq!(state.current.unwrap().recording_id.to_string(), next.id);
    assert_eq!(state.revision, 2);
    assert_eq!(f.server.requests("v1/sources").await.len(), 2);
    assert_eq!(f.server.requests("v1/audio").await.len(), 2);
    capture.after(capture.mark(), 0.1);
    assert!(capture::amplitude(&capture.recent(0.05), 440.) > 0.2);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 2,
                action: PlayerAction::Pause {},
            },
        )
        .await
        .unwrap();
    let previous = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 4,
                revision: 2,
                action: PlayerAction::Previous {},
            },
        )
        .await
        .unwrap();
    assert_eq!(previous.revision, 2);
    let current = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(current.current.unwrap().recording_id.to_string(), next.id);
    assert!(!current.desired_playing);
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|p| p.seek_id == Some(previous.id)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(f.server.requests("v1/sources").await.len(), 2);
    assert_eq!(f.server.requests("v1/audio").await.len(), 2);
    owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn an_all_failed_queue_ends_after_one_cached_and_one_refreshed_pass_per_occurrence() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.server
        .audio(
            axum::http::StatusCode::BAD_GATEWAY,
            Default::default(),
            Vec::new(),
        )
        .await;
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
                    recordings: vec![Uuid::parse_str(&f.source.recording_id).unwrap(); 2],
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
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(4),
        state.wait_for(|state| state.current.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(state.borrow().queue_len, 2);
    assert_eq!(state.borrow().revision, 3);
    assert!(!state.borrow().playing);
    assert!(!state.borrow().desired_playing);
    assert_eq!(
        state
            .borrow()
            .last_failure
            .as_ref()
            .unwrap()
            .entry
            .recording_id
            .to_string(),
        f.source.recording_id
    );
    owner.close().await.unwrap();
    assert_eq!(f.server.requests("v1/sources").await.len(), 3);
    assert_eq!(f.server.requests("v1/audio").await.len(), 4);
}
