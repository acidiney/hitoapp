use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::test_support::support;

use audio_fixture::AudioLibrary;
use hito_core::{DownloadRequest, DownloadState, DownloadStatus};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use uuid::Uuid;

const CLEANUP: &str = include_str!("../../src/downloads/cleanup.rs");
type IntentMetadata = (String, String, String, Option<String>, i64);

struct Measurement {
    steps: u64,
    plan: Vec<String>,
}

#[tokio::test]
async fn managed_source_cleanup_does_not_scan_inactive_download_intent_history() {
    for statistics in ["none", "before-growth", "after-growth"] {
        let library = AudioLibrary::start().await;
        library.ready().await;
        let accepted = library
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
        assert!(
            library
                .hito
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap()
        );
        library.hito.downloads.finish_downloads().await.unwrap();
        let ready = library
            .hito
            .downloads
            .status(&library.actor, &library.source.recording_id)
            .await
            .unwrap();
        assert!(matches!(ready.status, DownloadState::Ready));
        assert_eq!(ready.intent_id, accepted.intent_id);
        let source = ready.source_id.as_deref().unwrap();
        assert_ne!(source, library.source.id.as_str());
        let options = SqliteConnectOptions::new()
            .filename(library.directory.path().join("hito.db"))
            .foreign_keys(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let (transfer, file): (String, String) = sqlx::query_as(
            "SELECT intent.id,intent.file_id FROM download_retentions root
            JOIN download_intents intent ON intent.id=root.transfer_id WHERE root.id=?",
        )
        .bind(&accepted.intent_id)
        .fetch_one(&mut connection)
        .await
        .unwrap();
        history(&mut connection, &transfer, 1, 32).await;
        if statistics == "before-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let small = delete_and_rollback(&mut connection, source, &file, 32).await;
        assert_ready(&library, &ready).await;
        history(&mut connection, &transfer, 33, 20_000).await;
        if statistics == "after-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let large = delete_and_rollback(&mut connection, source, &file, 20_000).await;
        assert_ready(&library, &ready).await;
        connection.close().await.unwrap();
        eprintln!(
            "Managed-source cleanup DELETE statistics={statistics} inactive_intents=32->20000 VM_steps~small:{} large:{} plan={:?}",
            small.steps, large.steps, large.plan
        );
        assert!(
            large.steps < 10_000,
            "{statistics}: actual managed source DELETE scanned inactive intent history in {} VM steps: {:?}",
            large.steps,
            large.plan
        );
        assert!(
            large.steps <= small.steps + 2000,
            "{statistics}: inactive intent history grew actual managed source DELETE from {} to {} VM steps: {:?}",
            small.steps,
            large.steps,
            large.plan
        );
    }
}

#[tokio::test]
async fn managed_source_cleanup_does_not_scan_unrelated_catalog_recordings() {
    for statistics in ["none", "before-growth", "after-growth"] {
        let library = AudioLibrary::start().await;
        library.ready().await;
        let accepted = library
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
        assert!(
            library
                .hito
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap()
        );
        library.hito.downloads.finish_downloads().await.unwrap();
        let ready = library
            .hito
            .downloads
            .status(&library.actor, &library.source.recording_id)
            .await
            .unwrap();
        assert!(matches!(ready.status, DownloadState::Ready));
        assert_eq!(ready.intent_id, accepted.intent_id);
        let source = ready.source_id.as_deref().unwrap();
        assert_ne!(source, library.source.id.as_str());
        let options = SqliteConnectOptions::new()
            .filename(library.directory.path().join("hito.db"))
            .foreign_keys(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let file: String = sqlx::query_scalar(
            "SELECT intent.file_id FROM download_retentions root
            JOIN download_intents intent ON intent.id=root.transfer_id WHERE root.id=?",
        )
        .bind(&accepted.intent_id)
        .fetch_one(&mut connection)
        .await
        .unwrap();
        let initial: i64 = sqlx::query_scalar("SELECT count(*) FROM recordings")
            .fetch_one(&mut connection)
            .await
            .unwrap();
        catalog_history(
            &mut connection,
            &library.source.recording_id,
            1,
            32,
            initial,
        )
        .await;
        if statistics == "before-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let small = delete_and_rollback(&mut connection, source, &file, 0).await;
        assert_ready(&library, &ready).await;
        catalog_history(
            &mut connection,
            &library.source.recording_id,
            33,
            20_000,
            initial,
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
        let large = delete_and_rollback(&mut connection, source, &file, 0).await;
        assert_ready(&library, &ready).await;
        connection.close().await.unwrap();
        eprintln!(
            "Managed-source cleanup DELETE statistics={statistics} unrelated_recordings=32->20000 VM_steps~small:{} large:{} plan={:?}",
            small.steps, large.steps, large.plan
        );
        assert!(
            large.steps < 10_000,
            "{statistics}: actual managed source DELETE scanned unrelated catalog recordings in {} VM steps: {:?}",
            large.steps,
            large.plan
        );
        assert!(
            large.steps <= small.steps + 2000,
            "{statistics}: unrelated catalog recordings grew actual managed source DELETE from {} to {} VM steps: {:?}",
            small.steps,
            large.steps,
            large.plan
        );
    }
}

async fn catalog_history(
    connection: &mut SqliteConnection,
    recording: &str,
    first: i64,
    last: i64,
    initial: i64,
) {
    let inserted = sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO recordings(id,library_id,title,artist,provenance,metadata_source_id)
        SELECT printf('50000000-0000-4000-8000-%012x',n.i),r.library_id,'Owned unavailable diagnostic recording',r.artist,'provider_response',NULL
        FROM n CROSS JOIN recordings r WHERE r.id=?")
        .bind(first).bind(last).bind(recording).execute(&mut *connection).await.unwrap();
    assert_eq!(inserted.rows_affected(), (last - first + 1) as u64);
    let (total, unavailable): (i64, i64) = sqlx::query_as("SELECT count(*),coalesce(sum(id LIKE '50000000-0000-4000-8000-%' AND metadata_source_id IS NULL),0) FROM recordings")
        .fetch_one(&mut *connection).await.unwrap();
    assert_eq!((total, unavailable), (initial + last, last));
    foreign_keys(connection).await;
}

fn statement(prefix: &str) -> &'static str {
    let start = CLEANUP
        .find(prefix)
        .expect("the production cleanup statement must exist");
    let sql = &CLEANUP[start..];
    let sql = &sql[..sql.find('"').unwrap()];
    assert!(
        !sql.contains('\\'),
        "decode escaped production SQL before measuring it"
    );
    sql
}

async fn history(connection: &mut SqliteConnection, transfer: &str, first: i64, last: i64) {
    // Inactive diagnostic intents reuse the owned recording and its captured provider origin.
    let inserted = sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO download_intents(id,account_id,recording_id,library_id,access_grant_id,host_device,state,
            provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,
            service_id,service_revision,credential_digest,audio_generation)
        SELECT 'cleanup-history-' || n.i,i.account_id,i.recording_id,i.library_id,i.access_grant_id,i.host_device,'removed',
            i.provider_source_id,i.binding_id,i.binding_revision,i.remote_source_id,i.installation_id,i.installation_revision,
            i.service_id,i.service_revision,i.credential_digest,i.audio_generation
        FROM n CROSS JOIN download_intents i WHERE i.id=?")
        .bind(first).bind(last).bind(transfer).execute(&mut *connection).await.unwrap();
    assert_eq!(inserted.rows_affected(), (last - first + 1) as u64);
    let (total, removed): (i64, i64) =
        sqlx::query_as("SELECT count(*),coalesce(sum(state='removed'),0) FROM download_intents")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!((total, removed), (last + 1, last));
    foreign_keys(connection).await;
}

async fn foreign_keys(connection: &mut SqliteConnection) {
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
            .is_empty(),
        "the cleanup probe and its history must keep valid foreign keys"
    );
}

async fn metadata(connection: &mut SqliteConnection) -> Vec<IntentMetadata> {
    sqlx::query_as(
        "SELECT id,state,provider_source_id,file_id,generation FROM download_intents ORDER BY id",
    )
    .fetch_all(connection)
    .await
    .unwrap()
}

async fn catalog_metadata(connection: &mut SqliteConnection) -> Vec<String> {
    sqlx::query_scalar("SELECT json_array(rowid,id,library_id,release_id,title,artist,duration_ms,initial_file_checksum,
        provenance,metadata_source_id,metadata_observation_id,metadata_binding_id) FROM recordings ORDER BY rowid")
        .fetch_all(connection).await.unwrap()
}

async fn delete_and_rollback(
    connection: &mut SqliteConnection,
    source: &str,
    file: &str,
    history: i64,
) -> Measurement {
    foreign_keys(connection).await;
    let before = metadata(connection).await;
    let before_catalog = catalog_metadata(connection).await;
    assert_eq!(before.len() as i64, history + 1);
    let before_file: (String, Option<String>) =
        sqlx::query_as("SELECT state,source_id FROM download_files WHERE id=?")
            .bind(file)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(before_file, ("ready".into(), Some(source.into())));
    let mut tx = connection.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let deleting =
        sqlx::query("UPDATE download_files SET state='deleting' WHERE id=? AND state='ready'")
            .bind(file)
            .execute(&mut *tx)
            .await
            .unwrap();
    let detached = sqlx::query(statement(
        "UPDATE download_files SET state='deleted',source_id=NULL ",
    ))
    .bind(file)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(deleting.rows_affected(), 1);
    assert_eq!(detached.rows_affected(), 1);
    let delete = statement("DELETE FROM sources ");
    let explain = format!("EXPLAIN QUERY PLAN {delete}");
    let plan: Vec<String> = sqlx::query(&explain)
        .bind(source)
        .bind(file)
        .fetch_all(&mut *tx)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get("detail"))
        .collect();
    let steps = Arc::new(AtomicU64::new(0));
    let progress = Arc::clone(&steps);
    tx.lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            progress.fetch_add(100, Ordering::Relaxed) + 100 < 2_000_000
        });
    let deleted = sqlx::query(delete)
        .bind(source)
        .bind(file)
        .execute(&mut *tx)
        .await;
    tx.lock_handle().await.unwrap().remove_progress_handler();
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE id=?")
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let during = metadata(&mut tx).await;
    let during_catalog = catalog_metadata(&mut tx).await;
    foreign_keys(&mut tx).await;
    let rollback = tx.rollback().await;
    let restored: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE id=?")
        .bind(source)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let after_file: (String, Option<String>) =
        sqlx::query_as("SELECT state,source_id FROM download_files WHERE id=?")
            .bind(file)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    let after = metadata(connection).await;
    let after_catalog = catalog_metadata(connection).await;
    foreign_keys(connection).await;
    assert!(rollback.is_ok(), "the cleanup probe must roll back");
    assert!(
        deleted.is_ok(),
        "the actual source DELETE must finish within its VM abort budget: {:?}, plan={plan:?}",
        deleted.as_ref().err()
    );
    assert_eq!(deleted.unwrap().rows_affected(), 1);
    assert_eq!(remaining, 0);
    assert_eq!(restored, 1);
    assert_eq!(during, before, "source deletion must preserve every intent");
    assert_eq!(after, before, "rollback must preserve every intent");
    assert!(
        during_catalog == before_catalog,
        "source deletion must preserve every recording"
    );
    assert!(
        after_catalog == before_catalog,
        "rollback must preserve every recording"
    );
    assert_eq!(
        after_file, before_file,
        "rollback must restore the ready file"
    );
    Measurement {
        steps: steps.load(Ordering::Relaxed),
        plan,
    }
}

async fn assert_ready(library: &AudioLibrary, expected: &DownloadStatus) {
    let current = library
        .hito
        .downloads
        .status(&library.actor, &library.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(current.status, DownloadState::Ready));
    assert_eq!(
        serde_json::to_value(current).unwrap(),
        serde_json::to_value(expected).unwrap(),
        "rollback must preserve the original public download status and root"
    );
    let grant = library
        .hito
        .playback
        .playback_source(
            &library.actor,
            &library.source.recording_id,
            expected.source_id.as_deref().unwrap(),
        )
        .await
        .unwrap();
    assert!(grant.is_local());
    assert!(support::audio_bytes(grant.open_file().await.unwrap()) == library.original);
    assert!(std::fs::read(library.music.join("original.wav")).unwrap() == library.original);
    assert_eq!(library.server.requests("v1/audio").await.len(), 1);
}
