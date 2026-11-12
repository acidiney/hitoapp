use super::{connect, grow, seed};
use sqlx::{Connection, Row, SqliteConnection};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Statistics {
    Absent,
    Stale,
    Fresh,
}

#[tokio::test]
async fn metadata_artist_writes_keep_vm_work_linear_as_catalog_grows() {
    for statistics in [Statistics::Stale, Statistics::Absent, Statistics::Fresh] {
        let small = measure(128, statistics, false).await;
        let large = measure(512, statistics, false).await;
        for (operation, (small, large)) in ["insert", "update"]
            .into_iter()
            .zip(small.into_iter().zip(large))
        {
            assert!(
                large <= small * 5 + 1000,
                "{statistics:?} {operation}: metadata trigger VM work grew beyond linear allowance: small={small} large={large}"
            );
            assert!(
                large <= 512 * 400,
                "{statistics:?} {operation}: metadata trigger exceeded per-row VM allowance: {large}"
            );
        }
    }
}

#[tokio::test]
async fn upgrade_92_bounds_source_artist_writes_with_retained_statistics() {
    for statistics in [Statistics::Stale, Statistics::Absent, Statistics::Fresh] {
        let small = measure(128, statistics, true).await;
        let large = measure(512, statistics, true).await;
        for (small, large) in small.into_iter().zip(large) {
            assert!(large <= small * 5 + 1000);
            assert!(large <= 512 * 400);
        }
    }
}

async fn measure(count: i64, statistics: Statistics, upgrade: bool) -> [u64; 2] {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned.db");
    let mut connection = connect(&path, if upgrade { 92 } else { 93 }).await;
    seed(&mut connection).await;
    grow(&mut connection, 1, 1).await;
    if statistics == Statistics::Stale {
        sqlx::query("ANALYZE")
            .execute(&mut connection)
            .await
            .unwrap();
    }
    grow(&mut connection, 2, count).await;
    if statistics == Statistics::Fresh {
        sqlx::query("ANALYZE")
            .execute(&mut connection)
            .await
            .unwrap();
    }
    if statistics == Statistics::Absent {
        let tables: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name='sqlite_stat1'")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        assert_eq!(tables, 0);
    } else {
        let stats: String = sqlx::query_scalar(
            "SELECT stat FROM sqlite_stat1 WHERE idx='sources_identity_recording'",
        )
        .fetch_one(&mut connection)
        .await
        .unwrap();
        assert_eq!(
            stats.split(' ').next().unwrap().parse::<i64>().unwrap(),
            if statistics == Statistics::Stale {
                1
            } else {
                count
            }
        );
    }
    let prior_statistics = if statistics == Statistics::Absent {
        Vec::new()
    } else {
        sqlx::query_as::<_, (String, Option<String>, String)>(
            "SELECT tbl,idx,stat FROM sqlite_stat1 ORDER BY tbl,idx",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap()
    };
    connection.close().await.unwrap();
    let mut connection = connect(&path, 93).await;
    if statistics != Statistics::Absent {
        let current = sqlx::query_as::<_, (String, Option<String>, String)>(
            "SELECT tbl,idx,stat FROM sqlite_stat1 ORDER BY tbl,idx",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert_eq!(
            current, prior_statistics,
            "0093 must preserve existing planner statistics"
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        93
    );
    let trigger: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name='source_artist_insert'")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    let select = trigger
        .split("SELECT lower")
        .nth(1)
        .unwrap()
        .split(" ON CONFLICT")
        .next()
        .unwrap();
    let explain = format!(
        "EXPLAIN QUERY PLAN SELECT lower{}",
        select
            .replace("new.artist", "'Owned source artist'")
            .replace("new.source_id", "'source-1'")
    );
    let plan: Vec<String> = sqlx::query(&explain)
        .fetch_all(&mut connection)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>(3))
        .collect();
    assert_eq!(plan.len(), 2);
    assert!(plan[0].starts_with("SEARCH s USING COVERING INDEX sources_identity_recording (id=?)"));
    assert!(plan[1].starts_with("SEARCH r USING INDEX sqlite_autoindex_recordings_1 (id=?)"));
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    eprintln!(
        "source-artist upgrade={upgrade} sqlite={version} statistics={statistics:?} rows={count} plan={plan:?}"
    );
    let insert = work(&mut connection, "INSERT INTO source_metadata(source_id,title,artist,duration_ms) SELECT id,'Owned source title','Owned source artist',1000 FROM sources").await;
    let update = work(
        &mut connection,
        "UPDATE source_metadata SET artist='Owned updated artist'",
    )
    .await;
    eprintln!(
        "source-artist upgrade={upgrade} statistics={statistics:?} rows={count} insert_vm={insert} update_vm={update}"
    );
    let artists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM artists WHERE library_id='library'")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(artists, 3);
    connection.close().await.unwrap();
    [insert, update]
}

async fn work(connection: &mut SqliteConnection, statement: &str) -> u64 {
    let steps = Arc::new(AtomicU64::new(0));
    let progress = Arc::clone(&steps);
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            progress.fetch_add(100, Ordering::Relaxed);
            true
        });
    let result = sqlx::query(statement).execute(&mut *connection).await;
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    result.unwrap();
    steps.load(Ordering::Relaxed)
}
