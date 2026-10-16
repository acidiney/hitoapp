#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "fixtures/progress_events.rs"]
mod progress_events;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase};
use std::{collections::HashSet, time::Duration};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn blocked_storage_bounds_terminal_checkpoints_preserves_order_and_retries_the_same_operation()
 {
    let f = fixture::LocalPlayback::start().await;
    let events = progress_events::Events::default();
    let writer = events.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish(),
    );
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
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
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
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mut expected = Vec::new();
    let mut sequence = 2;
    for index in 0..80 {
        let position_ms = (index * 13) % 800;
        let id = Uuid::new_v4();
        let result = player
            .submit(
                &f.actor,
                PlayerCommand {
                    id,
                    lease: attached.lease,
                    sequence,
                    revision: 1,
                    action: PlayerAction::Seek { position_ms },
                },
            )
            .await;
        if result == Err(PlayerError::Busy) {
            break;
        }
        result.unwrap();
        expected.push(position_ms as i64);
        tokio::time::timeout(
            Duration::from_secs(3),
            position.wait_for(|position| position.seek_id == Some(id)),
        )
        .await
        .unwrap()
        .unwrap();
        sequence += 1;
    }
    assert!(!expected.is_empty() && expected.len() <= 64);
    assert_eq!(
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence,
                    revision: 1,
                    action: PlayerAction::Next,
                }
            )
            .await,
        Err(PlayerError::Busy)
    );
    tokio::time::timeout(
        Duration::from_secs(12),
        state.wait_for(|state| state.persistence_error == Some(PlayerError::Persistence)),
    )
    .await
    .unwrap()
    .unwrap();
    let first = events.started()[0];
    tokio::time::timeout(Duration::from_secs(3), events.started_after(1))
        .await
        .unwrap();
    assert_eq!(events.started()[1], first);
    for action in [
        PlayerAction::Pause,
        PlayerAction::Volume { level: 0 },
        PlayerAction::Clear,
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
            .await
            .unwrap();
        sequence += 1;
    }
    assert_eq!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .queue_len,
        0
    );
    transaction.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(8), owner.close())
        .await
        .unwrap()
        .unwrap();
    let journal = f.services.collection.events(&f.actor, 0).await.unwrap();
    assert!(!journal.has_more);
    assert_eq!(journal.operations.len(), expected.len());
    assert_eq!(
        journal
            .operations
            .iter()
            .map(|event| event.operation.id)
            .collect::<HashSet<_>>()
            .len(),
        expected.len()
    );
    assert_eq!(
        journal
            .operations
            .iter()
            .map(|event| event.device_id.as_str())
            .collect::<HashSet<_>>()
            .len(),
        1
    );
    let positions = journal
        .operations
        .iter()
        .map(|event| match event.operation.action {
            hito_core::Action::ProgressSet { position_ms, .. } => position_ms,
            _ => panic!("the native client wrote a non-progress operation"),
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, expected);
    assert_eq!(
        f.services
            .collection
            .activity(&f.actor)
            .await
            .unwrap()
            .history[0]
            .position_ms,
        *expected.last().unwrap()
    );
    pool.close().await;
}
