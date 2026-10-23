#![cfg(target_os = "linux")]

#[path = "support/correction_schema.rs"]
mod correction_schema;

use std::{borrow::Cow, fs::Permissions, os::unix::fs::PermissionsExt};

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use hito_community::{Config, Service, router};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    ConnectOptions, Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions,
};
use tower::ServiceExt;

const ADMIN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";
const OWNER: &str = "11111111-1111-4111-8111-111111111111";
const PEER: &str = "22222222-2222-4222-8222-222222222222";
const OWNER_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PEER_TOKEN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[tokio::test]
async fn corrections_upgrade_preserves_existing_rate_index_and_both_historical_migration_receipts()
{
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture
        .path()
        .join("private-existing-rate-index-upgrade-state");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, Permissions::from_mode(0o700)).unwrap();
    let database = directory.join("community.sqlite");
    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true)
        .foreign_keys(true)
        .disable_statement_logging();
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    for (contents, expected) in [
        (
            include_bytes!("../migrations/0001_community.sql").as_slice(),
            "fe71091f624f4447bfee87addfead488de6f9fe126cf612a9ead6b5f377a2f83",
        ),
        (
            include_bytes!("../migrations/0002_outcome_reporter_time.sql").as_slice(),
            "9429ddfac6953a991de7c268bb6433ee6c8c3bde3cf4d1b86acf85c7a63df19c",
        ),
        (
            include_bytes!("../migrations/0003_correction_suggestions.sql").as_slice(),
            "24627748a2866d782718053d95f395ee61e2442e23964d60dbcf215e1f76dc15",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(contents)), expected);
    }
    let legacy = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 2)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    legacy.run(&mut connection).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET rowid = version * 7, installed_on = CASE version WHEN 1 THEN '2001-02-03 04:05:06' ELSE '2002-03-04 05:06:07' END, execution_time = 100 + version")
        .execute(&mut connection).await.unwrap();
    for (rowid, id, token) in [(7_i64, OWNER, OWNER_TOKEN), (13, PEER, PEER_TOKEN)] {
        sqlx::query("INSERT INTO reporters(rowid,id,token_hash,created_at) VALUES(?,?,?,123456)")
            .bind(rowid)
            .bind(id)
            .bind(Sha256::digest(token.as_bytes()).to_vec())
            .execute(&mut connection)
            .await
            .unwrap();
    }
    for (rowid, reporter, submission, receipt, recording) in [
        (
            21_i64,
            OWNER,
            "10000000-0000-4000-8000-000000000001",
            "20000000-0000-4000-8000-000000000001",
            "42",
        ),
        (
            34,
            OWNER,
            "10000000-0000-4000-8000-000000000002",
            "20000000-0000-4000-8000-000000000002",
            "43",
        ),
        (
            55,
            PEER,
            "30000000-0000-4000-8000-000000000001",
            "40000000-0000-4000-8000-000000000001",
            "42",
        ),
        (
            89,
            PEER,
            "30000000-0000-4000-8000-000000000002",
            "40000000-0000-4000-8000-000000000002",
            "43",
        ),
    ] {
        sqlx::query("INSERT INTO outcomes(rowid,reporter_id,submission_id,receipt_id,provider,recording_id,created_at) VALUES(?,?,?,?,'jamendo',?,123457)").bind(rowid).bind(reporter).bind(submission).bind(receipt).bind(recording).execute(&mut connection).await.unwrap();
    }
    let before = snapshot(&mut connection).await;
    let schema_before = schema(&mut connection).await;
    let ledger_before = ledger(&mut connection).await;
    let keys_before = keys(&mut connection).await;
    assert_eq!(
        ledger_before
            .iter()
            .map(|row| (row.0, row.1))
            .collect::<Vec<_>>(),
        [(7, 1), (14, 2)]
    );
    assert_eq!(before.0.len(), 2);
    assert_eq!(before.1.len(), 4);
    connection.close().await.unwrap();
    std::fs::set_permissions(&database, Permissions::from_mode(0o600)).unwrap();

    let service = Service::open(Config {
        state_directory: directory,
        admin_token: ADMIN.to_owned(),
    })
    .await
    .unwrap();
    let app = router(service.clone());
    let mut connection = SqliteConnection::connect_with(&options.create_if_missing(false))
        .await
        .unwrap();
    assert!(
        snapshot(&mut connection).await == before,
        "The correction migration changed retained typed rows or rowids"
    );
    let mut expected = schema_before;
    expected.extend(correction_schema::objects());
    expected.sort();
    assert_eq!(schema(&mut connection).await, expected);
    correction_schema::assert_empty_structure(&mut connection).await;
    assert_eq!(keys(&mut connection).await, keys_before);
    let index: Vec<(i64, i64, String)> =
        sqlx::query_as("PRAGMA index_info(outcomes_reporter_created)")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        index,
        [(0, 0, "reporter_id".into()), (1, 5, "created_at".into())]
    );
    let after = ledger(&mut connection).await;
    assert_eq!(after.iter().map(|row| row.1).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(after[..2] == ledger_before);
    assert_eq!(
        after[2].0, 15,
        "The new migration receipt must append after the preserved historical rowids"
    );
    for (actual, migration) in after.iter().zip(all.iter()) {
        assert_eq!(actual.1, migration.version);
        assert_eq!(actual.2.as_str(), migration.description.as_ref());
        assert!(actual.4);
        assert!(actual.5.as_slice() == migration.checksum.as_ref());
    }
    let foreign_key_errors = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert!(foreign_key_errors.is_empty());
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    connection.close().await.unwrap();
    for (token, submission, receipt) in [
        (
            OWNER_TOKEN,
            "10000000-0000-4000-8000-000000000001",
            "20000000-0000-4000-8000-000000000001",
        ),
        (
            PEER_TOKEN,
            "30000000-0000-4000-8000-000000000001",
            "40000000-0000-4000-8000-000000000001",
        ),
    ] {
        let body = json!({"submission_id": submission, "source": {"provider": "jamendo", "recording_id": "42"}, "outcome": "successful_playback"});
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/outcomes")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"receipt_id": receipt, "submission_id": submission, "status": "accepted"})
        );
    }
    service.close().await;
}

type Reporters = Vec<(i64, String, Vec<u8>, i64)>;
type Outcomes = Vec<(i64, String, String, String, String, String, i64)>;
type Ledger = Vec<(i64, i64, String, String, bool, Vec<u8>, i64)>;
type Schema = Vec<(String, String, String, Option<String>)>;
type Keys = Vec<(i64, i64, String, String, String, String, String, String)>;

async fn snapshot(connection: &mut SqliteConnection) -> (Reporters, Outcomes) {
    (sqlx::query_as("SELECT rowid,id,token_hash,created_at FROM reporters ORDER BY rowid").fetch_all(&mut *connection).await.unwrap(), sqlx::query_as("SELECT rowid,reporter_id,submission_id,receipt_id,provider,recording_id,created_at FROM outcomes ORDER BY rowid").fetch_all(connection).await.unwrap())
}

async fn schema(connection: &mut SqliteConnection) -> Schema {
    sqlx::query_as(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name,tbl_name,sql",
    )
    .fetch_all(connection)
    .await
    .unwrap()
}

async fn ledger(connection: &mut SqliteConnection) -> Ledger {
    sqlx::query_as("SELECT rowid,version,description,installed_on,success,checksum,execution_time FROM _sqlx_migrations ORDER BY version").fetch_all(connection).await.unwrap()
}

async fn keys(connection: &mut SqliteConnection) -> Keys {
    sqlx::query_as("PRAGMA foreign_key_list(outcomes)")
        .fetch_all(connection)
        .await
        .unwrap()
}
