#![cfg(feature = "device")]
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_failed_final_checkpoint_reports_storage_failure_after_output_and_observations_close() {
    let f = fixture::LocalPlayback::start().await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
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
    let before = f.services.collection.events(&f.actor, 0).await.unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_progress BEFORE UPDATE ON listening_progress BEGIN SELECT RAISE(ABORT, 'fixture denied progress update'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), owner.close())
            .await
            .unwrap(),
        Err(PlayerError::Persistence)
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    let after = f.services.collection.events(&f.actor, 0).await.unwrap();
    assert_eq!(after.operations.len(), before.operations.len());
    pool.close().await;
}
