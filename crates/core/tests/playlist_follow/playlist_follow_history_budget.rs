use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

async fn start_work_counter(connection: &mut SqliteConnection) -> Arc<AtomicU64> {
    let steps = Arc::new(AtomicU64::new(0));
    let progress = Arc::clone(&steps);
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            progress.fetch_add(100, Ordering::Relaxed) + 100 < 2_000_000
        });
    steps
}

async fn finish_work_counter(connection: &mut SqliteConnection, steps: Arc<AtomicU64>) -> u64 {
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    steps.load(Ordering::Relaxed)
}

async fn latest_job_status(connection: &mut SqliteConnection) -> (String, u64) {
    let steps = start_work_counter(connection).await;
    let result = sqlx::query_scalar::<_, String>(
        "SELECT status FROM playlist_follow_jobs job WHERE job.playlist_id = ? ORDER BY job.created_at DESC,job.rowid DESC LIMIT 1",
    )
    .bind("playlist-0")
    .fetch_one(&mut *connection)
    .await;
    let work = finish_work_counter(connection, steps).await;
    assert!(
        result.is_ok(),
        "Latest follow status exceeded the query work budget or failed after {work} steps"
    );
    (result.unwrap(), work)
}

async fn prune_old_history(connection: &mut SqliteConnection) -> u64 {
    sqlx::query(
        "INSERT INTO playlist_follow_jobs(id,user_id,playlist_id,generation,status,created_at)
         VALUES ('job-old','account-0','playlist-0',1,'complete',199)",
    )
    .execute(&mut *connection)
    .await
    .unwrap();
    let steps = start_work_counter(connection).await;
    let result = sqlx::query("DELETE FROM playlist_follow_jobs WHERE rowid IN (
        SELECT job.rowid FROM playlist_follow_jobs job
        WHERE job.playlist_id = ? AND job.status IN ('complete','failed','cancelled')
            AND NOT EXISTS(SELECT 1 FROM playlist_follow_refresh_receipts receipt WHERE receipt.job_id = job.id)
        ORDER BY job.created_at DESC,job.rowid DESC LIMIT -1 OFFSET ?)")
        .bind("playlist-0")
        .bind(32)
        .execute(&mut *connection)
        .await;
    let work = finish_work_counter(connection, steps).await;
    assert!(
        result.is_ok(),
        "Follow history pruning exceeded the query work budget or failed after {work} steps"
    );
    assert_eq!(result.unwrap().rows_affected(), 1);
    let retained: Vec<(String, String)> = sqlx::query_as(
        "SELECT id,status FROM playlist_follow_jobs WHERE playlist_id = ? ORDER BY rowid",
    )
    .bind("playlist-0")
    .fetch_all(connection)
    .await
    .unwrap();
    let expected: Vec<(String, String)> = (0..32)
        .map(|i| {
            (
                format!("job-playlist-0-{i}"),
                if i == 31 { "failed" } else { "complete" }.into(),
            )
        })
        .collect();
    assert_eq!(retained, expected);
    work
}

#[tokio::test]
async fn latest_follow_status_work_does_not_grow_with_other_playlists_history() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(directory.path().join("follow-history.db"))
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    sqlx::migrate!("./migrations")
        .run(&mut connection)
        .await
        .unwrap();
    let mut tx = connection.begin().await.unwrap();
    sqlx::raw_sql(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 15)
         INSERT INTO users(id,username,password_hash)
         SELECT 'account-' || i,'account-' || i,'fixture' FROM n;
         INSERT INTO plugin_services(id,name,base_url)
         VALUES ('service','Owned history fixture','https://provider.invalid/');
         INSERT INTO plugin_installations(id,user_id,service_id,manifest)
         SELECT 'installation-' || substr(id,9),id,'service',
             '{\"id\":\"org.hito.history\",\"name\":\"Owned history fixture\",\"protocol_version\":1,\"capabilities\":[\"playlists\"]}'
         FROM users;
         INSERT INTO playlists(id,user_id,name,kind)
         VALUES ('playlist-0','account-0','Owned history fixture','followed');
         INSERT INTO playlist_follows(playlist_id,user_id,installation_id,installation_revision,
             service_id,service_revision,snapshot_installation_revision,snapshot_service_revision,
             provider,provider_playlist_id,snapshot,content_digest,
             attribution,interval_seconds,next_refresh_at)
         VALUES ('playlist-0','account-0','installation-0',1,'service',1,1,1,'org.hito.history',
             'playlist-0','snapshot',printf('%064d',0),'{\"label\":\"Owned history fixture\"}',3600,3600);
         WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 31)
         INSERT INTO playlist_follow_jobs(id,user_id,playlist_id,generation,status,created_at)
         SELECT 'job-playlist-0-' || i,'account-0','playlist-0',1,
             CASE WHEN i = 31 THEN 'failed' ELSE 'complete' END,200 FROM n;",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let (small_status, small_steps) = latest_job_status(&mut connection).await;
    assert_eq!(small_status, "failed");
    let small_prune_steps = prune_old_history(&mut connection).await;

    let mut tx = connection.begin().await.unwrap();
    sqlx::raw_sql(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < 4095)
         INSERT INTO playlists(id,user_id,name,kind)
         SELECT 'playlist-' || i,'account-' || (i / 256),'Owned history fixture','followed' FROM n;
         INSERT INTO playlist_follows(playlist_id,user_id,installation_id,installation_revision,
             service_id,service_revision,snapshot_installation_revision,snapshot_service_revision,
             provider,provider_playlist_id,snapshot,content_digest,
             attribution,interval_seconds,next_refresh_at)
         SELECT p.id,p.user_id,installation.id,1,'service',1,1,1,'org.hito.history',p.id,'snapshot',
             printf('%064d',0),'{\"label\":\"Owned history fixture\"}',3600,3600
         FROM playlists p JOIN plugin_installations installation ON installation.user_id = p.user_id
         WHERE p.id != 'playlist-0';
         WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 31)
         INSERT INTO playlist_follow_jobs(id,user_id,playlist_id,generation,status,created_at)
         SELECT 'job-' || p.id || '-' || n.i,p.user_id,p.id,1,'complete',200
         FROM playlists p CROSS JOIN n WHERE p.id != 'playlist-0';",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users),(SELECT count(*) FROM playlist_follows),
            (SELECT count(*) FROM playlist_follow_jobs)",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(counts, (16, 4096, 131_072));
    let foreign_key_errors: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert!(foreign_key_errors.is_empty());
    let (large_status, large_steps) = latest_job_status(&mut connection).await;
    assert_eq!(large_status, "failed");
    eprintln!("Follow status query steps small={small_steps} large={large_steps} jobs=131072");
    assert!(small_steps < 2_000);
    assert!(
        large_steps <= small_steps + 2_000,
        "Unrelated follow history increased lookup work from {small_steps} to {large_steps} steps"
    );
    let large_prune_steps = prune_old_history(&mut connection).await;
    eprintln!(
        "Follow history prune steps~ small={small_prune_steps} large={large_prune_steps} jobs=131073"
    );
    assert!(small_prune_steps < 10_000);
    assert!(
        large_prune_steps <= small_prune_steps + 2_000,
        "Unrelated follow history increased pruning work from {small_prune_steps} to {large_prune_steps} steps"
    );
    connection.close().await.unwrap();
}
