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
    owner.close().with_subscriber(dispatch).await.unwrap();
    let logs = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("player_command_accepted"));
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
