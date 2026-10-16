#[path = "fixtures/authority_outage.rs"]
mod authority_outage;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn queue_workflow_logs_retain_safe_correlation_and_exclude_private_input() {
    let f = fixture::LocalPlayback::start().await;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    support::synchronize(
        &f.services,
        &f.actor,
        vec![hito_core::Action::RecordingTitleSet {
            recording_id: recording,
            title: "private-player-title-sentinel".into(),
        }],
    )
    .await;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer = captured.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || Sink(writer.clone()))
            .finish(),
    );
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::start(f.services.clone()));
    let seen = player
        .inspect(&f.actor)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    let attached = player
        .claim(&f.actor, seen.epoch)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    let id = Uuid::new_v4();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id,
                lease: attached.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![recording],
                    start: 0,
                },
            },
        )
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    player
        .queue(&f.actor, attached.lease, 1, 0, 10)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    let mut observed = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let outage = authority_outage::Outage::start(f.directory.path()).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        observed.wait_for(|state| state.phase == hito_playback::PlayerPhase::Suspended),
    )
    .await
    .unwrap()
    .unwrap();
    outage.restore().await;
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        observed.wait_for(|state| state.phase != hito_playback::PlayerPhase::Suspended),
    )
    .await
    .unwrap()
    .unwrap();
    owner.close().with_subscriber(dispatch).await.unwrap();
    let logs = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("player_command_accepted"));
    assert!(logs.contains("player_authority_suspended"));
    assert!(logs.contains("player_authority_restored"));
    let events = logs
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        events
            .iter()
            .any(|event| event["span"]["action"] == "replace")
    );
    assert!(
        events
            .iter()
            .any(|event| event["fields"]["elapsed_ms"].is_number())
    );
    assert!(logs.contains("player_owner_closed"));
    assert!(
        events
            .iter()
            .any(|event| event["span"]["player_owner"].is_number()),
        "owner correlation is missing"
    );
    assert!(logs.contains(&id.to_string()));
    assert!(logs.contains(&f.actor.user().id));
    for private in [
        &f.token,
        &attached.lease.to_string(),
        "private-player-title-sentinel",
        f.directory.path().to_str().unwrap(),
        "indexing test password",
    ] {
        assert!(
            !logs.contains(private),
            "a private sentinel reached workflow logs"
        );
    }
}

#[cfg(feature = "device")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn native_output_transitions_and_joined_cleanup_have_correlated_redacted_logs() {
    let f = fixture::LocalPlayback::start().await;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer = captured.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || Sink(writer.clone()))
            .finish(),
    );
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::native(f.services.clone()));
    let attached = player
        .claim(
            &f.actor,
            player
                .inspect(&f.actor)
                .with_subscriber(dispatch.clone())
                .await
                .unwrap()
                .epoch,
        )
        .with_subscriber(dispatch.clone())
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
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    for (sequence, action) in [
        (2, PlayerAction::Shuffle {}),
        (3, PlayerAction::Repeat {}),
        (4, PlayerAction::Resume {}),
    ] {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence,
                    revision: 1,
                    action,
                },
            )
            .with_subscriber(dispatch.clone())
            .await
            .unwrap();
    }
    let mut state = player
        .subscribe(&f.actor, attached.lease)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        state.wait_for(|state| state.phase == hito_playback::PlayerPhase::Ready && state.playing),
    )
    .await
    .unwrap()
    .unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        position.wait_for(|p| p.position_ms >= 100),
    )
    .await
    .unwrap()
    .unwrap();
    owner.close().with_subscriber(dispatch).await.unwrap();
    let logs = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    let events = logs
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let transition = events
        .iter()
        .find(|event| event["fields"]["event"] == "player_output_changed")
        .expect("native transitions need workflow logs");
    assert!(transition["span"]["player_owner"].is_number());
    assert!(transition["fields"]["selection"].is_number());
    for name in ["player_shuffle_changed", "player_repeat_changed"] {
        let changed = events
            .iter()
            .find(|event| event["fields"]["event"] == name)
            .expect("mode changes need workflow logs");
        assert!(changed["span"]["player_owner"].is_number());
    }
    let played = events
        .iter()
        .find(|event| event["fields"]["event"] == "player_entry_played")
        .expect("played occurrences need workflow logs");
    assert!(played["span"]["player_owner"].is_number());
    assert_eq!(played["fields"]["retained"], 1);
    assert!(Uuid::parse_str(played["fields"]["entry_id"].as_str().unwrap()).is_ok());
    assert!(logs.contains("native_session_finished"));
    assert!(logs.contains("audio_player_closed"));
    assert!(logs.contains("player_owner_closed"));
    for private in [
        &f.token,
        &attached.lease.to_string(),
        f.directory.path().to_str().unwrap(),
        "indexing test password",
    ] {
        assert!(
            !logs.contains(private),
            "a private sentinel reached native logs"
        );
    }
}
