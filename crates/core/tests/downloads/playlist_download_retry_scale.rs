use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::test_support::support;

use audio_fixture::AudioLibrary;
use hito_core::{Action, DownloadRequest, DownloadState};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use uuid::Uuid;

const RETRY: &str = include_str!("../../src/downloads/playlist_retry.rs");
const DUPLICATE: &str = "20000000-0000-4000-8000-000000000000";
const NEXT: &str = "30000000-0000-4000-8000-000000000000";
const MISSING: &str = "40000000-0000-4000-8000-000000000000";

enum Argument {
    Text(String),
    Number(i64),
    Null,
}

struct Probe {
    name: &'static str,
    sql: &'static str,
    arguments: Vec<Argument>,
    expected: Vec<Vec<String>>,
}

struct Measurement {
    steps: u64,
    plan: Vec<String>,
}

#[tokio::test]
async fn playlist_retry_queries_bound_duplicate_batches_and_unrelated_failure_history() {
    let scan = statement("let mut recordings:");
    let authority = statement("let authority:");
    for statistics in ["none", "before-growth", "after-growth"] {
        let library = AudioLibrary::start().await;
        let playlist = Uuid::new_v4();
        support::synchronize(
            &library.hito,
            &library.actor,
            vec![Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Owned retry diagnostic playlist".into(),
            }],
        )
        .await;
        let receipt = library
            .hito
            .downloads
            .create(
                &library.actor,
                &library.source.recording_id,
                DownloadRequest {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        let options = SqliteConnectOptions::new()
            .filename(library.directory.path().join("hito.db"))
            .foreign_keys(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let migration: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _sqlx_migrations WHERE version=32 AND success=1",
        )
        .fetch_one(&mut connection)
        .await
        .unwrap();
        assert_eq!(migration, 1);
        let transfer: String =
            sqlx::query_scalar("SELECT transfer_id FROM download_retentions WHERE id=?")
                .bind(receipt.intent_id)
                .fetch_one(&mut connection)
                .await
                .unwrap();
        let (library_id, grant, host): (String, String, String) = sqlx::query_as(
            "SELECT library_id,access_grant_id,host_device FROM download_intents WHERE id=?",
        )
        .bind(&transfer)
        .fetch_one(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE download_intents SET state='failed',error='transfer_failed' WHERE id=?",
        )
        .bind(&transfer)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query("UPDATE download_jobs SET status='failed' WHERE intent_id=?")
            .bind(&transfer)
            .execute(&mut connection)
            .await
            .unwrap();
        assert!(matches!(
            library
                .hito
                .downloads
                .status(&library.actor, &library.source.recording_id)
                .await
                .unwrap()
                .status,
            DownloadState::Failed
        ));
        for recording in [DUPLICATE, NEXT, MISSING] {
            sqlx::query("INSERT INTO recordings(id,library_id,title,artist,provenance)
                SELECT ?,library_id,'Owned retry diagnostic recording',artist,'provider_response' FROM recordings WHERE id=?")
                .bind(recording).bind(&library.source.recording_id).execute(&mut connection).await.unwrap();
        }
        let playlist = playlist.to_string();
        sqlx::query("INSERT INTO playlist_entries(playlist_id,id,recording_id,position) VALUES(?,'next-recording',?,100001)")
            .bind(&playlist).bind(NEXT).execute(&mut connection).await.unwrap();
        grow(&mut connection, &playlist, &transfer, 1, 32, 1, 32).await;
        if statistics == "before-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let small_cutoff: i64 =
            sqlx::query_scalar("SELECT max(sequence) FROM download_terminal_failures")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        assert_eq!(small_cutoff, 33);
        let cases = |cutoff| {
            let scan_case = |name, cursor, expected| Probe {
                name,
                sql: scan,
                arguments: vec![Argument::Text(playlist.clone()), cursor],
                expected,
            };
            let authority_case = |name, recording: &str, expected| Probe {
                name,
                sql: authority,
                arguments: vec![
                    Argument::Text(library.actor.user().id.clone()),
                    Argument::Text(recording.into()),
                    Argument::Text(host.clone()),
                    Argument::Number(cutoff),
                ],
                expected,
            };
            vec![
                scan_case(
                    "first-raw-batch",
                    Argument::Null,
                    vec![vec![DUPLICATE.into()]; 32],
                ),
                scan_case(
                    "after-duplicate-tail",
                    Argument::Text(DUPLICATE.into()),
                    vec![vec![NEXT.into()]],
                ),
                scan_case("exhausted", Argument::Text(NEXT.into()), vec![]),
                authority_case("missing-authority", MISSING, vec![]),
                authority_case(
                    "matching-authority",
                    &library.source.recording_id,
                    vec![vec![library_id.clone(), grant.clone()]],
                ),
            ]
        };
        let mut small = Vec::new();
        for probe in cases(small_cutoff) {
            small.push(measure(&mut connection, &probe).await);
        }
        grow(
            &mut connection,
            &playlist,
            &transfer,
            33,
            100_000,
            33,
            20_000,
        )
        .await;
        if statistics == "after-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let large_cutoff: i64 =
            sqlx::query_scalar("SELECT max(sequence) FROM download_terminal_failures")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        assert_eq!(large_cutoff, 20_001);
        let large_cases = cases(large_cutoff);
        let mut large = Vec::new();
        for probe in &large_cases {
            large.push(measure(&mut connection, probe).await);
        }
        for ((probe, small), large) in large_cases.iter().zip(&small).zip(&large) {
            eprintln!(
                "Playlist retry query={} statistics={statistics} VM_steps~small:{} large:{} plan={:?}",
                probe.name, small.steps, large.steps, large.plan
            );
            assert!(
                large.steps < 10_000,
                "{statistics} {} scanned duplicate entries or unrelated failure history in {} VM steps: {:?}",
                probe.name,
                large.steps,
                large.plan
            );
            assert!(
                large.steps <= small.steps + 2000,
                "{statistics} {} grew from {} to {} VM steps: {:?}",
                probe.name,
                small.steps,
                large.steps,
                large.plan
            );
        }
        assert!(library.server.requests("v1/audio").await.is_empty());
        connection.close().await.unwrap();
    }
}

fn statement(scope: &str) -> &'static str {
    let start = RETRY.find(scope).expect("the production query must exist");
    let start = start + RETRY[start..].find('"').unwrap() + 1;
    let sql = &RETRY[start..];
    let sql = &sql[..sql.find('"').unwrap()];
    assert!(sql.starts_with("SELECT"));
    assert!(
        !sql.contains('\\'),
        "decode escaped production SQL before measuring it"
    );
    sql
}

async fn grow(
    connection: &mut SqliteConnection,
    playlist: &str,
    transfer: &str,
    first_entry: i64,
    last_entry: i64,
    first_failure: i64,
    last_failure: i64,
) {
    // These diagnostic clones retain valid FKs; provider authority and transfer workers never run.
    let entries = sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO playlist_entries(playlist_id,id,recording_id,position)
        SELECT ?,printf('duplicate-%06d',n.i),?,n.i FROM n",
    )
    .bind(first_entry)
    .bind(last_entry)
    .bind(playlist)
    .bind(DUPLICATE)
    .execute(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        entries.rows_affected(),
        (last_entry - first_entry + 1) as u64
    );
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO recordings(id,library_id,title,artist,provenance)
        SELECT printf('10000000-0000-4000-8000-%012x',n.i),i.library_id,'Owned failed retry diagnostic recording',r.artist,'provider_response'
        FROM n CROSS JOIN download_intents i JOIN recordings r ON r.id=i.recording_id WHERE i.id=?")
        .bind(first_failure).bind(last_failure).bind(transfer).execute(&mut *connection).await.unwrap();
    let failures = sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO download_intents(id,account_id,recording_id,library_id,access_grant_id,host_device,state,
            provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,
            service_id,service_revision,credential_digest,audio_generation,error)
        SELECT '!retry-history-' || n.i,i.account_id,printf('10000000-0000-4000-8000-%012x',n.i),
            i.library_id,i.access_grant_id,i.host_device,'failed',i.provider_source_id,i.binding_id,i.binding_revision,
            i.remote_source_id,i.installation_id,i.installation_revision,i.service_id,i.service_revision,
            i.credential_digest,i.audio_generation,'transfer_failed'
        FROM n CROSS JOIN download_intents i WHERE i.id=?")
        .bind(first_failure).bind(last_failure).bind(transfer).execute(&mut *connection).await.unwrap();
    assert_eq!(
        failures.rows_affected(),
        (last_failure - first_failure + 1) as u64
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM playlist_entries WHERE playlist_id=?")
            .bind(playlist)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(count, last_entry + 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM download_terminal_failures")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(count, last_failure + 1);
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(enabled, 1);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(connection)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn measure(connection: &mut SqliteConnection, probe: &Probe) -> Measurement {
    let explain = format!("EXPLAIN QUERY PLAN {}", probe.sql);
    let mut plan = sqlx::query(&explain);
    for argument in &probe.arguments {
        plan = match argument {
            Argument::Text(value) => plan.bind(value),
            Argument::Number(value) => plan.bind(*value),
            Argument::Null => plan.bind(None::<String>),
        };
    }
    let plan: Vec<String> = plan
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
            progress.fetch_add(100, Ordering::Relaxed) + 100 < 5_000_000
        });
    let mut query = sqlx::query(probe.sql);
    for argument in &probe.arguments {
        query = match argument {
            Argument::Text(value) => query.bind(value),
            Argument::Number(value) => query.bind(*value),
            Argument::Null => query.bind(None::<String>),
        };
    }
    let result = query.fetch_all(&mut *tx).await;
    tx.lock_handle().await.unwrap().remove_progress_handler();
    tx.rollback().await.unwrap();
    assert!(
        result.is_ok(),
        "{} must finish within its VM abort budget: {:?}, plan={plan:?}",
        probe.name,
        result.as_ref().err()
    );
    let result: Vec<Vec<String>> = result
        .unwrap()
        .iter()
        .map(|row| {
            (0..row.len())
                .map(|column| row.get::<String, _>(column))
                .collect()
        })
        .collect();
    assert_eq!(result, probe.expected, "{} result", probe.name);
    Measurement {
        steps: steps.load(Ordering::Relaxed),
        plan,
    }
}
