#[path = "fixtures/maintenance_barrier.rs"]
mod barrier;
#[path = "fixtures/maintenance.rs"]
mod fixture;

use futures_util::FutureExt;
use sqlx::Connection;
use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};
use tracing_subscriber::prelude::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborting_the_migration_observer_preserves_the_owner_through_connection_close() {
    let barrier = barrier::MaintenanceBarrier::default();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(barrier.clone()))
        .unwrap();
    let library = Arc::new(fixture::Library::version_29("http://127.0.0.1:9/").await);
    let before = fixture::Snapshot::read(&library.database).await;
    let files_before = library.files();
    let release = barrier.arm(library.clone());
    let database = library.database.clone();
    let mut observer = tokio::spawn(async move { hito_core::migrate_existing(database, 29).await });
    let mut observer_joined = false;
    let observation = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(3), barrier.entered())
            .await
            .expect(
                "the owned migration must finish its history read before observer cancellation",
            );
        assert!(!barrier.finished());
        observer.abort();
        let result = tokio::time::timeout(Duration::from_secs(1), &mut observer)
            .await
            .expect("the migration observer must acknowledge cancellation before barrier release");
        observer_joined = true;
        assert!(matches!(result, Err(error) if error.is_cancelled()));
        assert!(!barrier.finished());
    })
    .catch_unwind()
    .await;
    drop(release);
    if !observer_joined {
        observer.abort();
        let _ = observer.await;
    }
    let completion = tokio::time::timeout(Duration::from_secs(10), barrier.completed())
        .await
        .expect("the detached maintenance owner must finish migration and await connection close");
    assert!(completion.complete);
    assert!(completion.close_complete);
    assert!(
        !barrier.timed_out(),
        "the test must release its SQL barrier before the fuse"
    );
    if let Err(panic) = observation {
        std::panic::resume_unwind(panic);
    }

    before.assert_preserved(&library.database).await;
    assert!(library.files() == files_before);
    let mut database = fixture::connect(&library.database).await;
    let applied: Result<Vec<(i64, Vec<u8>, bool)>, sqlx::Error> = async {
        let mut transaction = database.begin_with("BEGIN IMMEDIATE").await?;
        let rows = sqlx::query_as(
            "SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&mut *transaction)
        .await;
        transaction.rollback().await?;
        rows
    }
    .await;
    database.close().await.unwrap();
    let applied = applied.unwrap();
    let migrations = fixture::migrations();
    assert_eq!(applied.len(), migrations.iter().count());
    for (actual, expected) in applied.iter().zip(migrations.iter()) {
        assert_eq!(actual.0, expected.version);
        assert!(actual.1.as_slice() == expected.checksum.as_ref());
        assert!(actual.2);
    }
    assert!(applied.last().unwrap().0 > 29);
}
