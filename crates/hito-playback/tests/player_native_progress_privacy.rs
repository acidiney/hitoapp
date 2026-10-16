#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "fixtures/progress_events.rs"]
mod progress_events;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_core::Credentials;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pending_old_account_progress_never_changes_the_new_accounts_activity_or_error_state() {
    let f = fixture::LocalPlayback::start().await;
    f.services
        .identity
        .create_user(
            &f.actor,
            Credentials {
                username: "progress-listener".into(),
                password: "private progress listener password".into(),
            },
        )
        .await
        .unwrap();
    let session = f
        .services
        .identity
        .login(Credentials {
            username: "progress-listener".into(),
            password: "private progress listener password".into(),
        })
        .await
        .unwrap();
    let another = f
        .services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
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
        (2, 1, PlayerAction::Resume {}),
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
    let mut old = player.subscribe(&f.actor, attached.lease).await.unwrap();
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
                action: PlayerAction::Pause {},
            },
        )
        .await
        .unwrap();
    let operation = tokio::time::timeout(Duration::from_secs(3), events.started_after(before))
        .await
        .unwrap();
    player
        .claim(&another, player.inspect(&another).await.unwrap().epoch)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while old.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(12), events.started_after(before + 1))
            .await
            .unwrap(),
        operation
    );
    let state = player.inspect(&another).await.unwrap().state.unwrap();
    assert_eq!(state.persistence_error, None);
    assert_eq!(state.queue_len, 0);
    assert_eq!(state.volume, 1000);
    assert!(
        f.services
            .collection
            .activity(&another)
            .await
            .unwrap()
            .history
            .is_empty()
    );
    transaction.rollback().await.unwrap();
    owner.close().await.unwrap();
    let history = f.services.collection.activity(&f.actor).await.unwrap();
    assert!(history.history[0].position_ms >= 150);
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
        f.services
            .collection
            .events(&another, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    assert!(player.inspect(&another).await.is_err());
    pool.close().await;
}
