#[allow(dead_code)]
#[path = "../fixtures/source_preparation_query_plans.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::Fixture;
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use uuid::Uuid;

const CLEANUP: &str = include_str!("../../src/downloads/cleanup.rs");

type Preparation = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
    i64,
);

#[tokio::test]
async fn deleting_an_unrelated_download_source_does_not_scan_preparation_history() {
    for statistics in ["none", "before-growth", "after-growth"] {
        let fixture = Fixture::start().await;
        let mut connection = open(&fixture).await;
        let source = Uuid::new_v4().to_string();
        let file = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,ready) VALUES (?,?,'download',?,0)")
            .bind(&source).bind(&fixture.recording).bind(&file)
            .execute(&mut connection).await.unwrap();
        history(&mut connection, &fixture.preparation, 1, 32).await;
        if statistics == "before-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = open(&fixture).await;
        let small = delete_and_rollback(&mut connection, &source, &file, 32, statistics).await;
        history(&mut connection, &fixture.preparation, 33, 20_000).await;
        if statistics == "after-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = open(&fixture).await;
        let large = delete_and_rollback(&mut connection, &source, &file, 20_000, statistics).await;
        assert!(
            large <= small + 2000,
            "{statistics}: unrelated source cleanup grew from {small} to {large} VM steps with terminal preparation history"
        );
        connection.close().await.unwrap();
    }
}

async fn open(fixture: &Fixture) -> SqliteConnection {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(enabled, 1);
    connection
}

async fn history(connection: &mut SqliteConnection, preparation: &str, first: i64, last: i64) {
    let inserted = sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i + 1 FROM n WHERE i < ?)
        INSERT INTO source_preparations(id,user_id,recording_id,source_id,library_id,access_grant,
            binding_id,binding_revision,provider_recording_id,remote_source_id,installation_id,installation_revision,
            service_id,service_revision,provider,credential,audio_generation,phase,submission_state,remote_outcome,
            remote_handle,created_at,next_check_at)
        SELECT 'fk-history-' || n.i,p.user_id,p.recording_id,p.source_id,p.library_id,p.access_grant,
            p.binding_id,p.binding_revision,p.provider_recording_id,p.remote_source_id,p.installation_id,p.installation_revision,
            p.service_id,p.service_revision,p.provider,p.credential,p.audio_generation,'failed','submitted','failed',
            'owned-terminal-handle-' || n.i,p.created_at,NULL
        FROM n CROSS JOIN source_preparations p WHERE p.id = ?")
        .bind(first).bind(last).bind(preparation).execute(&mut *connection).await.unwrap();
    assert_eq!(inserted.rows_affected(), (last - first + 1) as u64);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM source_preparations")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(count, last + 1);
    foreign_keys(connection).await;
}

async fn foreign_keys(connection: &mut SqliteConnection) {
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(connection)
        .await
        .unwrap();
    assert!(
        violations.is_empty(),
        "all preparation and source targets must have valid foreign keys"
    );
}

async fn metadata(connection: &mut SqliteConnection) -> Vec<Preparation> {
    sqlx::query_as(
        "SELECT id,recording_id,source_id,phase,submission_state,remote_outcome,
        progress_basis_points,eta_seconds,updated_at FROM source_preparations ORDER BY id",
    )
    .fetch_all(connection)
    .await
    .unwrap()
}

async fn delete_and_rollback(
    connection: &mut SqliteConnection,
    source: &str,
    file: &str,
    history: i64,
    statistics: &str,
) -> u64 {
    let start = CLEANUP
        .find("DELETE FROM sources ")
        .expect("production cleanup must delete its managed source");
    let sql = &CLEANUP[start..];
    let sql = &sql[..sql.find('"').unwrap()];
    assert!(
        !sql.contains('\\'),
        "decode escaped production SQL before measuring it"
    );
    let before = metadata(connection).await;
    assert_eq!(before.len() as i64, history + 1);
    let plan: Vec<String> = sqlx::query(&format!("EXPLAIN QUERY PLAN {sql}"))
        .bind(source)
        .bind(file)
        .fetch_all(&mut *connection)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get("detail"))
        .collect();
    let mut tx = connection.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let steps = Arc::new(AtomicU64::new(0));
    let progress = Arc::clone(&steps);
    tx.lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            progress.fetch_add(100, Ordering::Relaxed) + 100 < 1_000_000
        });
    let deleted = sqlx::query(sql)
        .bind(source)
        .bind(file)
        .execute(&mut *tx)
        .await;
    tx.lock_handle().await.unwrap().remove_progress_handler();
    let steps = steps.load(Ordering::Relaxed);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE id = ?")
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let during = metadata(&mut tx).await;
    let rollback = tx.rollback().await;
    let restored: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE id = ?")
        .bind(source)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let after = metadata(connection).await;
    foreign_keys(connection).await;
    assert!(
        rollback.is_ok(),
        "the measured cleanup transaction must roll back"
    );
    assert!(
        deleted.is_ok(),
        "managed source cleanup failed or exceeded the VM abort budget: {:?}",
        deleted.as_ref().err()
    );
    assert_eq!(deleted.unwrap().rows_affected(), 1);
    assert_eq!(remaining, 0);
    assert_eq!(restored, 1);
    assert_eq!(
        during, before,
        "deleting an unrelated source must preserve preparation metadata"
    );
    assert_eq!(after, before, "rollback must preserve preparation metadata");
    eprintln!(
        "Managed-source DELETE statistics={statistics} history={history} VM_steps~{steps} plan={plan:?}"
    );
    if history == 20_000 {
        assert!(
            !plan
                .iter()
                .any(|detail| detail.contains("SCAN source_preparations")),
            "foreign-key enforcement must look up the deleted source without scanning terminal preparation history: {plan:?}"
        );
        assert!(
            steps < 10_000,
            "managed source cleanup exceeded 10,000 VM steps with {statistics}: {steps}"
        );
    }
    steps
}
