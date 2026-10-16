#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "fixtures/progress_events.rs"]
mod progress_events;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn canceling_close_observation_keeps_admitted_progress_owned_until_commit_and_join() {
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
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.position_ms >= 150),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while f
            .services
            .collection
            .activity(&f.actor)
            .await
            .unwrap()
            .history
            .is_empty()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let before = events.started().len();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 1,
                action: PlayerAction::Pause,
            },
        )
        .await
        .unwrap();
    let operation = tokio::time::timeout(Duration::from_secs(3), events.started_after(before))
        .await
        .unwrap();
    let mut closing = tokio::spawn(owner.close());
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut closing)
            .await
            .is_err()
    );
    closing.abort();
    assert!(closing.await.unwrap_err().is_cancelled());
    transaction.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), events.joined())
        .await
        .unwrap();
    let activity = f.services.collection.activity(&f.actor).await.unwrap();
    assert!(activity.history[0].position_ms >= 150);
    let journal = f.services.collection.events(&f.actor, 0).await.unwrap();
    assert_eq!(
        journal
            .operations
            .iter()
            .filter(|event| event.operation.id == operation)
            .count(),
        1
    );
    assert!(
        journal
            .operations
            .iter()
            .all(|event| event.device_id.starts_with("native-") && event.device_id != "playback")
    );
    pool.close().await;
}
