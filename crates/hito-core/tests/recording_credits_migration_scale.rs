use sqlx::{
    Connection, SqliteConnection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

#[tokio::test]
async fn upgrade_of_a_large_reopened_catalog_stays_within_a_linear_work_budget() {
    let directory = tempfile::tempdir().unwrap();
    let seed = directory.path().join("before-upgrade.db");
    let options = SqliteConnectOptions::new()
        .filename(&seed)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 20).cloned().collect()),
        ..Migrator::DEFAULT
    }
    .run(&mut connection)
    .await
    .unwrap();
    let mut tx = connection.begin().await.unwrap();
    sqlx::raw_sql(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 9)
         INSERT INTO users(id,username,password_hash) SELECT 'user-' || i, 'user-' || i, 'fixture' FROM n;
         INSERT INTO libraries(id,name) VALUES ('shared','Large prior library');
         INSERT INTO library_members SELECT 'shared', id FROM users;
         WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 99999)
         INSERT INTO recordings(id,library_id,title,artist,provenance)
         SELECT 'track-' || i, 'shared', 'Track ' || i, 'Performer ' || (i / 10), 'provider_response' FROM n;
         INSERT INTO account_artists(user_id,id,library_id,name)
         SELECT u.id, u.id || '-' || a.id, a.library_id, a.name FROM users u CROSS JOIN artists a;",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    connection.close().await.unwrap();

    for analyzed in [false, true] {
        let path = directory.path().join(format!("upgrade-{analyzed}.db"));
        std::fs::copy(&seed, &path).unwrap();
        let options = options.clone().filename(&path).create_if_missing(false);
        if analyzed {
            let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
            connection.close().await.unwrap();
        }
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let steps = Arc::new(AtomicU64::new(0));
        let progress = Arc::clone(&steps);
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_progress_handler(1000, move || {
                progress.fetch_add(1000, Ordering::Relaxed) + 1000 < 250_000_000
            });
        let started = Instant::now();
        let result = all.run(&mut connection).await;
        connection
            .lock_handle()
            .await
            .unwrap()
            .remove_progress_handler();
        let work = steps.load(Ordering::Relaxed);
        eprintln!(
            "Recording-credit upgrade analyzed={analyzed} steps~{work} elapsed={:?}",
            started.elapsed()
        );
        assert!(
            result.is_ok(),
            "Reopened catalog upgrade exceeded its work budget or failed: {result:?}"
        );
        assert!(work < 250_000_000);
        let counts: Vec<(String, i64)> = sqlx::query_as(
            "SELECT user_id, COUNT(*) FROM account_recording_credits GROUP BY user_id",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert_eq!(counts.len(), 10);
        assert!(counts.iter().all(|(_, count)| *count == 100_000));
        let wrong: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM account_recording_credits credit
             JOIN recordings r ON r.id = credit.recording_id
             JOIN account_artists artist ON artist.user_id = credit.user_id AND artist.id = credit.artist_id
             JOIN artists shared ON shared.library_id = r.library_id AND shared.name = r.artist COLLATE BINARY
             WHERE artist.library_id != r.library_id OR artist.name != r.artist COLLATE BINARY
                OR artist.id != credit.user_id || '-' || shared.id",
        )
        .fetch_one(&mut connection)
        .await
        .unwrap();
        assert_eq!(wrong, 0);
        let errors: Vec<(String, Option<i64>, String, i64)> =
            sqlx::query_as("PRAGMA foreign_key_check")
                .fetch_all(&mut connection)
                .await
                .unwrap();
        assert!(errors.is_empty());
        connection.close().await.unwrap();
    }
}
