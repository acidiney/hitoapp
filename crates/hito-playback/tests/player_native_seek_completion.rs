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
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_seek_accepted_as_audio_finishes_applies_without_stalling_or_advancing_the_queue() {
    let f = fixture::LocalPlayback::start().await;
    let gate = event_gate::EventGate::new("player_command_accepted");
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
    for (sequence, revision, action) in [
        (
            1,
            0,
            PlayerAction::Replace {
                recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
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
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.position_ms >= 300),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    let id = Uuid::new_v4();
    gate.arm();
    let submitting = tokio::spawn({
        let player = player.clone();
        let actor = f.actor.clone();
        async move {
            player
                .submit(
                    &actor,
                    PlayerCommand {
                        id,
                        lease: attached.lease,
                        sequence: 3,
                        revision: 1,
                        action: PlayerAction::Seek { position_ms: 0 },
                    },
                )
                .with_subscriber(dispatch)
                .await
                .unwrap()
        }
    });
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    capture.after(capture.mark(), 1.2);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    gate.release();
    let receipt = tokio::time::timeout(Duration::from_secs(3), submitting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.revision, 1);
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.seek_id == Some(id)),
    )
    .await
    .unwrap()
    .unwrap();
    let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(state.current.unwrap().recording_id.to_string(), f.recording);
    assert_eq!(state.selection, 1);
    assert!(state.desired_playing);
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    owner.close().await.unwrap();
}
