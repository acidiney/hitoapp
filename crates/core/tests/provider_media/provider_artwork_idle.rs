use hito_core::{CoreConfig, HostDeviceId};
use sqlx::{Connection, Executor, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::Duration;

#[tokio::test]
async fn idle_artwork_poll_does_not_wait_for_an_unrelated_writer() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let hito = hito_core::open(CoreConfig::new(
        database.clone(),
        directory.path().join("music"),
        HostDeviceId::new("idle-artwork-poll").unwrap(),
    ))
    .await
    .unwrap();
    let mut writer =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(database))
            .await
            .unwrap();
    writer.execute("BEGIN IMMEDIATE").await.unwrap();
    let mut polling = {
        let catalog = hito.catalog.clone();
        tokio::spawn(async move {
            catalog
                .run_next_artwork_job_until(std::future::pending())
                .await
        })
    };
    let observed = tokio::time::timeout(Duration::from_secs(1), &mut polling).await;

    writer.execute("ROLLBACK").await.unwrap();
    writer.close().await.unwrap();
    let (finished_while_writer_held, result) = match observed {
        Ok(result) => (true, result),
        Err(_) => (false, polling.await),
    };
    let claimed = result.unwrap().unwrap();

    assert!(
        finished_while_writer_held,
        "an idle artwork worker must not wait for an unrelated SQLite writer"
    );
    assert!(!claimed);
}
