use hito_core::test_support::provider_audio as audio_fixture;

use audio_fixture::AudioLibrary;
use hito_core::{DownloadRequest, DownloadState};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use uuid::Uuid;

const ADMISSION: &str = include_str!("../src/downloads/playlist_admission.rs");

struct Measurement {
    steps: u64,
    plan: Vec<String>,
}

#[tokio::test]
async fn playlist_failed_transfer_lookup_stays_bounded_with_other_failed_recordings() {
    let scope = ADMISSION
        .find("let failed:")
        .expect("the production failed-transfer probe must exist");
    let call = scope + ADMISSION[scope..].find("sqlx::query_scalar(").unwrap();
    let start = call + ADMISSION[call..].find('"').unwrap() + 1;
    let sql = &ADMISSION[start..];
    let sql = &sql[..sql.find('"').unwrap()];
    assert!(sql.starts_with("SELECT EXISTS"));
    assert!(
        !sql.contains('\\'),
        "decode escaped production SQL before measuring it"
    );

    for statistics in ["none", "before-growth", "after-growth"] {
        let library = AudioLibrary::start().await;
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
            "SELECT count(*) FROM _sqlx_migrations WHERE version=31 AND success=1",
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
        let status = library
            .hito
            .downloads
            .status(&library.actor, &library.source.recording_id)
            .await
            .unwrap();
        assert!(matches!(status.status, DownloadState::Failed));
        let account = &library.actor.user().id;
        let matching = &library.source.recording_id;
        let missing = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO recordings(id,library_id,title,artist,provenance)
            SELECT ?,library_id,'Owned candidate without a transfer',artist,'provider_response' FROM recordings WHERE id=?")
            .bind(&missing).bind(matching).execute(&mut connection).await.unwrap();
        history(&mut connection, &transfer, 1, 32).await;
        if statistics == "before-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let cases = [(&missing, 0_i64), (matching, 1_i64)];
        let mut small = Vec::new();
        for (recording, expected) in cases {
            small.push(
                probe(
                    &mut connection,
                    sql,
                    (account, recording, &status.host_device_id),
                    expected,
                )
                .await,
            );
        }
        history(&mut connection, &transfer, 33, 20_000).await;
        if statistics == "after-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let mut large = Vec::new();
        for (recording, expected) in cases {
            large.push(
                probe(
                    &mut connection,
                    sql,
                    (account, recording, &status.host_device_id),
                    expected,
                )
                .await,
            );
        }
        for ((label, small), large) in ["missing", "matching"].into_iter().zip(&small).zip(&large) {
            eprintln!(
                "Playlist failed probe target={label} statistics={statistics} VM_steps~small:{} large:{} plan={:?}",
                small.steps, large.steps, large.plan
            );
            assert!(
                large.steps < 10_000,
                "{statistics} {label}: failed-transfer lookup scanned unrelated history in {} VM steps: {:?}",
                large.steps,
                large.plan
            );
            assert!(
                large.steps <= small.steps + 2000,
                "{statistics} {label}: other failed recordings grew query work from {} to {} VM steps",
                small.steps,
                large.steps
            );
        }
        assert!(library.server.requests("v1/audio").await.is_empty());
        connection.close().await.unwrap();
    }
}

async fn history(connection: &mut SqliteConnection, transfer: &str, first: i64, last: i64) {
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO recordings(id,library_id,title,artist,provenance)
        SELECT printf('10000000-0000-4000-8000-%012x',n.i),i.library_id,'Owned failed diagnostic recording',r.artist,'provider_response'
        FROM n CROSS JOIN download_intents i JOIN recordings r ON r.id=i.recording_id WHERE i.id=?")
        .bind(first).bind(last).bind(transfer).execute(&mut *connection).await.unwrap();
    // Diagnostic intents keep valid FKs and reuse an owned origin without running transfer authority or workers.
    // Their IDs sort before the public transfer so an account/state scan cannot find the matching control early.
    let inserted = sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO download_intents(id,account_id,recording_id,library_id,access_grant_id,host_device,state,
            provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,
            service_id,service_revision,credential_digest,audio_generation,error)
        SELECT '!failed-history-' || n.i,i.account_id,printf('10000000-0000-4000-8000-%012x',n.i),
            i.library_id,i.access_grant_id,i.host_device,'failed',i.provider_source_id,i.binding_id,i.binding_revision,
            i.remote_source_id,i.installation_id,i.installation_revision,i.service_id,i.service_revision,
            i.credential_digest,i.audio_generation,'transfer_failed'
        FROM n CROSS JOIN download_intents i WHERE i.id=?")
        .bind(first).bind(last).bind(transfer).execute(&mut *connection).await.unwrap();
    assert_eq!(inserted.rows_affected(), (last - first + 1) as u64);
    let failed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM download_intents WHERE state='failed'")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(failed, last + 1);
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

async fn probe(
    connection: &mut SqliteConnection,
    sql: &str,
    target: (&str, &str, &str),
    expected: i64,
) -> Measurement {
    let (account, recording, host) = target;
    let plan: Vec<String> = sqlx::query(&format!("EXPLAIN QUERY PLAN {sql}"))
        .bind(account)
        .bind(recording)
        .bind(host)
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
            progress.fetch_add(100, Ordering::Relaxed) + 100 < 2_000_000
        });
    let result = sqlx::query_scalar::<_, i64>(sql)
        .bind(account)
        .bind(recording)
        .bind(host)
        .fetch_one(&mut *tx)
        .await;
    tx.lock_handle().await.unwrap().remove_progress_handler();
    tx.rollback().await.unwrap();
    assert!(
        result.is_ok(),
        "the actual failed probe must finish within its VM abort budget: {:?}",
        result.as_ref().err()
    );
    assert_eq!(result.unwrap(), expected);
    Measurement {
        steps: steps.load(Ordering::Relaxed),
        plan,
    }
}
