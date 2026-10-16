#![cfg(feature = "device")]
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "fixtures/event_gate.rs"]
mod event_gate;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn completion_before_the_first_playing_observation_still_saves_the_finished_position() {
    let f = fixture::LocalPlayback::start().await;
    let gate = event_gate::EventGate::new("player_controls_applied");
    let _release = gate.guard();
    let writer = gate.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish(),
    );
    let capture = capture::Capture::open();
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::native(f.services.clone()));
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
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        f.services
            .collection
            .activity(&f.actor)
            .await
            .unwrap()
            .history
            .is_empty()
    );
    gate.arm();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::Resume,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    capture.after(capture.mark(), 1.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    gate.release();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.current.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    owner.close().await.unwrap();
    let activity = f.services.collection.activity(&f.actor).await.unwrap();
    assert_eq!(activity.history.len(), 1);
    assert_eq!(activity.history[0].id, f.recording);
    assert_eq!(activity.history[0].position_ms, 1_000);
    let journal = f.services.collection.events(&f.actor, 0).await.unwrap();
    assert_eq!(journal.operations.len(), 1);
}
