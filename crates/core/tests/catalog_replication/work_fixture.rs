use crate::Services;
use sqlx::{Connection, SqliteConnection};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub(super) async fn install_meter(services: &Services) -> Arc<AtomicU64> {
    let steps = Arc::new(AtomicU64::new(0));
    let mut idle = services.catalog_replication.reader.lock().await;
    let connection = idle.as_mut().unwrap();
    let progress = steps.clone();
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            progress.fetch_add(100, Ordering::Relaxed) + 100 < 500_000_000
        });
    steps
}

async fn seed(connection: &mut SqliteConnection, library: &str, rows: usize) {
    let template: (Option<String>, String, i64) = sqlx::query_as(
        "SELECT release_id,artist,duration_ms FROM recordings WHERE library_id=? LIMIT 1",
    )
    .bind(library)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    let mut tx = connection.begin().await.unwrap();
    let work = std::sync::Arc::new(AtomicU64::new(0));
    tx.lock_handle()
        .await
        .unwrap()
        .set_progress_handler(1000, move || {
            work.fetch_add(1000, Ordering::Relaxed) + 1000 < 100_000_000
        });
    let started = std::time::Instant::now();
    eprintln!("fixture_stage=seed_recordings rows={rows}");
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i+1<?)
        INSERT INTO recordings(id,library_id,release_id,title,artist,duration_ms)
        SELECT printf('legacy-recording-%06d',i),?,?,printf('Track %06d',?-i),?,? FROM n",
    )
    .bind(rows as i64)
    .bind(library)
    .bind(template.0)
    .bind(rows as i64)
    .bind(template.1)
    .bind(template.2)
    .execute(&mut *tx)
    .await
    .expect("seed recordings within fixture work budget");
    eprintln!("fixture_stage=seed_sources rows={rows}");
    sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,ready)
        SELECT 'source-'||id,id,'local','source-'||id,0 FROM recordings WHERE library_id=? AND id LIKE 'legacy-recording-%'")
        .bind(library).execute(&mut *tx).await.expect("seed sources within fixture work budget");
    eprintln!("fixture_stage=seed_metadata rows={rows}");
    sqlx::query(
        "INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms)
        SELECT 'source-'||r.id,r.release_id,r.title,r.artist,r.duration_ms FROM recordings r
        WHERE r.library_id=? AND r.id LIKE 'legacy-recording-%'",
    )
    .bind(library)
    .execute(&mut *tx)
    .await
    .expect("seed metadata within fixture work budget");
    tx.lock_handle().await.unwrap().remove_progress_handler();
    eprintln!(
        "fixture_stage=seed_complete rows={rows} elapsed={:?}",
        started.elapsed()
    );
    tx.commit().await.unwrap();
}

pub(super) async fn populate(services: &Services, library: &str, rows: usize, statistics: &str) {
    let mut connection =
        SqliteConnection::connect_with(&services.catalog_replication.store.pool.connect_options())
            .await
            .unwrap();
    let mut tables: Vec<String> = Vec::new();
    if statistics == "stale" {
        sqlx::query("ANALYZE")
            .execute(&mut connection)
            .await
            .unwrap();
        tables=sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE name IN ('sqlite_stat1','sqlite_stat4') ORDER BY name").fetch_all(&mut connection).await.unwrap();
        for name in &tables {
            sqlx::query(&format!(
                "CREATE TEMP TABLE saved_{name} AS SELECT * FROM {name}"
            ))
            .execute(&mut connection)
            .await
            .unwrap();
            sqlx::query(&format!("DELETE FROM {name}"))
                .execute(&mut connection)
                .await
                .unwrap();
        }
        sqlx::query("ANALYZE sqlite_schema")
            .execute(&mut connection)
            .await
            .unwrap();
    }
    seed(&mut connection, library, rows - 1).await;
    grow_coordinates(&mut connection, library).await;
    assert_cardinalities(&mut connection, library, rows).await;
    for name in &tables {
        sqlx::query(&format!("DELETE FROM {name}"))
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query(&format!("INSERT INTO {name} SELECT * FROM saved_{name}"))
            .execute(&mut connection)
            .await
            .unwrap();
        let equal:bool=sqlx::query_scalar(&format!("SELECT (SELECT count(*) FROM {name})=(SELECT count(*) FROM saved_{name}) AND NOT EXISTS(SELECT * FROM {name} EXCEPT SELECT * FROM saved_{name}) AND NOT EXISTS(SELECT * FROM saved_{name} EXCEPT SELECT * FROM {name})"))
            .fetch_one(&mut connection).await.unwrap();
        assert!(
            equal,
            "captured planner statistics must be restored exactly"
        );
        eprintln!("fixture_statistics_table={name} restored_exact=true");
    }
    if statistics == "fresh" {
        sqlx::query("ANALYZE")
            .execute(&mut connection)
            .await
            .unwrap();
    } else if statistics == "stale" {
        sqlx::query("ANALYZE sqlite_schema")
            .execute(&mut connection)
            .await
            .unwrap();
    }
    if statistics != "absent" {
        let stat: String =
            sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='recordings_library'")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        let estimated: i64 = stat.split_whitespace().next().unwrap().parse().unwrap();
        let expected = if statistics == "stale" {
            1
        } else {
            rows as i64
        };
        assert_eq!(
            estimated, expected,
            "the export must see the requested planner statistics"
        );
        eprintln!(
            "fixture_statistics={statistics} estimated_recordings={estimated} actual_recordings={rows}"
        );
    }
    connection.close().await.unwrap();
}

async fn grow_coordinates(connection: &mut SqliteConnection, library: &str) {
    let mut tx = connection.begin().await.unwrap();
    let work = std::sync::Arc::new(AtomicU64::new(0));
    tx.lock_handle()
        .await
        .unwrap()
        .set_progress_handler(1000, move || {
            work.fetch_add(1000, Ordering::Relaxed) + 1000 < 100_000_000
        });
    let started = std::time::Instant::now();
    eprintln!("fixture_stage=grow_coordinates");
    sqlx::query("INSERT INTO releases(id,library_id,title,artist,identity_key)
        SELECT 'release-'||id,library_id,'Album '||id,artist,'history-'||id FROM recordings WHERE library_id=? AND id LIKE 'legacy-recording-%'")
        .bind(library).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE recordings SET release_id='release-'||id WHERE library_id=? AND id LIKE 'legacy-recording-%'")
        .bind(library).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "UPDATE source_metadata SET release_id='release-'||substr(source_id,length('source-')+1)
        WHERE source_id LIKE 'source-legacy-recording-%'",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO local_files(source_id,library_id,folder_id,relative_path,path,scan_root,size_bytes)
        SELECT 'source-'||r.id,r.library_id,owned.folder_id,json_quote('history/'||r.id||'.wav'),owned.scan_root||'/history/'||r.id||'.wav',owned.scan_root,owned.size_bytes
        FROM recordings r
        CROSS JOIN (SELECT * FROM local_files WHERE library_id=? ORDER BY source_id LIMIT 1) owned
        WHERE r.library_id=? AND r.id LIKE 'legacy-recording-%'")
        .bind(library).bind(library).execute(&mut *tx).await.unwrap();
    tx.lock_handle().await.unwrap().remove_progress_handler();
    eprintln!(
        "fixture_stage=grow_coordinates_complete elapsed={:?}",
        started.elapsed()
    );
    tx.commit().await.unwrap();
}

async fn assert_cardinalities(connection: &mut SqliteConnection, library: &str, expected: usize) {
    let work = Arc::new(AtomicU64::new(0));
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(1000, move || {
            work.fetch_add(1000, Ordering::Relaxed) + 1000 < 100_000_000
        });
    let joins = "FROM recordings r INDEXED BY recordings_library CROSS JOIN sources s INDEXED BY sources_recording ON s.recording_id=r.id CROSS JOIN source_metadata m INDEXED BY sqlite_autoindex_source_metadata_1 ON m.source_id=s.id WHERE r.library_id=?1";
    let sql = format!(
        "SELECT (SELECT count(*) FROM recordings WHERE library_id=?1),(SELECT count(DISTINCT m.release_id) {joins}),(SELECT count(*) {joins}),(SELECT count(*) FROM local_files WHERE library_id=?1)"
    );
    let counts: (i64, i64, i64, i64) = sqlx::query_as(&sql)
        .bind(library)
        .fetch_one(&mut *connection)
        .await
        .expect("verify exact fixture cardinalities within a bounded SQL work budget");
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(enabled, 1, "fixture foreign keys must remain enabled");
    let violations: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    assert!(
        violations.is_empty(),
        "all fixture foreign keys must be valid"
    );
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    eprintln!(
        "fixture_expected={expected} recordings={} distinct_releases={} source_metadata={} local_files={} foreign_keys_valid=true",
        counts.0, counts.1, counts.2, counts.3
    );
    let expected = expected as i64;
    assert_eq!(
        counts,
        (expected, expected, expected, expected),
        "fixture cardinalities must be verified before measuring SQL work"
    );
}
