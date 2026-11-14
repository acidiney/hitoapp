#![cfg(feature = "device")]
use hito_audio::test_support as audio_support;
use hito_audio::test_support::capture;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::test_support::support;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use hito_testkit::logs::LogCapture;
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn exhausted_sources_report_failure_after_three_bounded_attempts_and_continue_the_queue() {
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
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
    let (player, owner) = async { Player::native(f.hito.clone()) }
        .with_subscriber(dispatch)
        .await;
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
    let final_logs = logs.events();
    let acquisition_demands = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "player_acquisition_started")
        .map(|event| event["fields"]["request_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let demand = *acquisition_demands
        .first()
        .expect("the failed occurrence must start an acquisition");
    let acquisitions = acquisition_demands
        .iter()
        .filter(|request_id| **request_id == demand)
        .count();
    assert_eq!(
        acquisitions, 1,
        "the occurrence must keep one acquisition demand"
    );
    assert!(
        acquisition_demands
            .iter()
            .skip(1)
            .all(|request_id| *request_id != demand),
        "a later queue occurrence must have its own acquisition demand"
    );
    let audio_requests = f.server.requests("v1/audio").await;
    assert_eq!(audio_requests.len(), 4);
    assert!(
        audio_requests.iter().all(|(_, query)| {
            query["_method"] == "GET"
                && query["source_id"].as_str() == Some("audio:/1?private=value&other=bad")
        }),
        "each audio open must be a GET for the owned failing source"
    );
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
    let audio_requests = f.server.requests("v1/audio").await;
    assert_eq!(audio_requests.len(), 4);
    assert!(
        audio_requests.iter().all(|(_, query)| {
            query["_method"] == "GET"
                && query["source_id"].as_str() == Some("audio:/1?private=value&other=bad")
        }),
        "paused Previous must not add an audio open"
    );
    owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn an_all_failed_queue_ends_after_three_bounded_attempts_per_occurrence() {
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
    let audio_requests = f.server.requests("v1/audio").await;
    assert_eq!(audio_requests.len(), 8);
    assert!(
        audio_requests.iter().all(|(_, query)| {
            query["_method"] == "GET"
                && query["source_id"].as_str() == Some("audio:/1?private=value&other=bad")
        }),
        "each bounded attempt must open only the owned failing source"
    );
}
