#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;
#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;

use futures_util::FutureExt;
use hito_core::{
    CoreConfig, HostDeviceId, ResolveSource,
    test_support::{database_owner::DatabaseOwner, hang_guard::hang_guard},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{
    borrow::Cow,
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

const TOKEN: &str = "owned-failure-upgrade-session";
const REQUEST: &str = "98000000-0000-4000-8000-000000000001";

#[tokio::test]
async fn failure_upgrade_preserves_populated_history_and_replays_legacy_unknown_decisions() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let database_path = directory.path().join("failure-upgrade.db");
    let options = SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = hang_guard("owned populated failure upgrade fixture", async {
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let all = sqlx::migrate!("./migrations");
        through(&all, 36).run(&mut connection).await.unwrap();
        sqlx::raw_sql(include_str!("../fixtures/source_resolution_upgrade.sql"))
            .execute(&mut connection).await.unwrap();
        sqlx::raw_sql(include_str!("../fixtures/source_resolution_evidence_upgrade.sql"))
            .execute(&mut connection).await.unwrap();
        through(&all, 54).run(&mut connection).await.unwrap();
        sqlx::raw_sql(include_str!("../fixtures/source_resolution_unavailable_upgrade.sql"))
            .execute(&mut connection).await.unwrap();
        through(&all, 96).run(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO source_resolutions(id,user_id,recording_id,target_digest,choice_revision,outcome,completed_at) SELECT '98000000-0000-4000-8000-000000000002',user_id,recording_id,target_digest,choice_revision,'provider_unavailable',1401 FROM source_resolutions WHERE id='unavailable-no-match'")
            .execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO source_resolution_receipts(user_id,request_id,recording_id,resolution_id) VALUES ('migration-owner',?,'evidence-recording','98000000-0000-4000-8000-000000000002')")
            .bind(REQUEST).execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO sessions(token_hash,user_id,expires_at,created_at,last_seen_at) VALUES (?,'migration-owner',4102444800,unixepoch(),unixepoch())")
            .bind(format!("{:x}", Sha256::digest(TOKEN.as_bytes())))
            .execute(&mut connection).await.unwrap();
        connection
    }).await;
    let before = snapshot::capture(&mut connection, 96).await;
    let old_schema = schema::schema(&mut connection).await;
    connection.close().await.unwrap();
    let options = options.create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    through(&sqlx::migrate!("./migrations"), 98)
        .run(&mut connection)
        .await
        .unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    let expected = old_schema
        .into_iter()
        .map(|mut object| {
            if object.0 == "table" && object.1 == "source_resolutions" {
                object.3 = object
                    .3
                    .map(|ddl| schema::playback_failure_schema::with_failure(&ddl));
            }
            object
        })
        .collect::<Vec<_>>();
    assert_eq!(schema::schema(&mut connection).await, expected);
    let invented: i64 =
        sqlx::query_scalar("SELECT count(*) FROM source_resolutions WHERE failure IS NOT NULL")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(invented, 0);
    assert_guards(&mut connection).await;
    snapshot::assert_preserved(&mut connection, &before).await;
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .unwrap()
            .is_empty()
    );
    connection.close().await.unwrap();
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    let ledger: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT version,checksum FROM _sqlx_migrations WHERE success=1 ORDER BY version",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    let expected = sqlx::migrate!("./migrations")
        .iter()
        .map(|migration| (migration.version, migration.checksum.to_vec()))
        .collect::<Vec<_>>();
    assert_eq!(ledger, expected);
    assert_eq!(ledger.len(), 97);
    assert_eq!(ledger.last().unwrap().0, 98);
    assert!(ledger.iter().all(|(version, _)| *version != 97));
    connection.close().await.unwrap();

    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let database = DatabaseOwner::retain(directory);
    let mut config = CoreConfig::new(
        database_path,
        music,
        HostDeviceId::new("failure-upgrade").unwrap(),
    );
    database.attach(&mut config);
    let hito = hang_guard(
        "owned upgraded public Core",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let observed = tokio::time::timeout(
        Duration::from_secs(3),
        AssertUnwindSafe(async {
            let actor = hito.identity.authenticate(TOKEN).await.unwrap();
            let request_id = Uuid::parse_str(REQUEST).unwrap();
            let first = hito
                .catalog
                .resolve_source_until(
                    &actor,
                    "evidence-recording",
                    ResolveSource { request_id },
                    std::future::pending(),
                )
                .await
                .unwrap();
            let second = hito
                .catalog
                .resolve_source_until(
                    &actor,
                    "evidence-recording",
                    ResolveSource { request_id },
                    std::future::pending(),
                )
                .await
                .unwrap();
            [
                serde_json::to_value(first).unwrap(),
                serde_json::to_value(second).unwrap(),
            ]
        })
        .catch_unwind(),
    )
    .await;
    let playback = hito.playback.close().await;
    let replicas = hito.replicas.close().await;
    let replication = hito.catalog_replication.close().await;
    drop(hito);
    let retired = database.retire(Duration::from_secs(3)).await;
    assert!(playback.is_ok() && replicas.is_ok() && replication.is_ok());
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    let [first, second] = match observed {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("legacy public receipt replay must settle within its behavior deadline"),
    };
    assert_eq!(first, second);
    assert_eq!(first["outcome"], json!("provider_unavailable"));
    assert_eq!(first["failure"], Value::Null);
}

fn through(all: &Migrator, version: i64) -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

async fn assert_guards(connection: &mut SqliteConnection) {
    let mut transaction = connection.begin().await.unwrap();
    for failure in [
        "{}",
        "{\"reason\":\"provider_unavailable\"}",
        "{\"reason\":\"provider_unavailable\",\"retryable\":false}",
        "{\"reason\":\"provider_refused\",\"retryable\":true}",
        "{\"reason\":\"unknown\",\"retryable\":false}",
        "{\"reason\":\"provider_refused\",\"retryable\":0}",
        "{\"reason\":\"provider_refused\",\"retryable\":false,\"notice\":\"owned\"}",
        "{",
    ] {
        assert!(sqlx::query("UPDATE source_resolutions SET outcome='provider_unavailable',completed_at=1401,lease_token=NULL,lease_until=NULL,failure=? WHERE id='unavailable-pending'")
            .bind(failure).execute(&mut *transaction).await.is_err());
    }
    let oversized = format!(
        "{{\"reason\":\"provider_refused\",\"retryable\":false}}{}",
        " ".repeat(128)
    );
    assert!(sqlx::query("UPDATE source_resolutions SET outcome='provider_unavailable',completed_at=1401,lease_token=NULL,lease_until=NULL,failure=? WHERE id='unavailable-pending'")
        .bind(oversized).execute(&mut *transaction).await.is_err());
    for outcome in [
        "ready",
        "no_suitable_source",
        "protected_choice",
        "cancelled",
    ] {
        assert!(sqlx::query("UPDATE source_resolutions SET outcome=?,completed_at=1401,lease_token=NULL,lease_until=NULL,failure=? WHERE id='unavailable-pending'")
            .bind(outcome).bind("{\"reason\":\"provider_refused\",\"retryable\":false}")
            .execute(&mut *transaction).await.is_err());
    }
    assert!(
        sqlx::query("UPDATE source_resolutions SET failure=? WHERE id='unavailable-pending'")
            .bind("{\"reason\":\"provider_refused\",\"retryable\":false}")
            .execute(&mut *transaction)
            .await
            .is_err()
    );
    for (reason, retryable) in [
        ("provider_unavailable", true),
        ("provider_refused", false),
        ("credentials_required", false),
        ("invalid_provider_response", false),
    ] {
        let failure = json!({"reason":reason,"retryable":retryable}).to_string();
        sqlx::query("SAVEPOINT owned_failure_guard")
            .execute(&mut *transaction)
            .await
            .unwrap();
        let changed = sqlx::query("UPDATE source_resolutions SET outcome='provider_unavailable',completed_at=1401,lease_token=NULL,lease_until=NULL,failure=? WHERE id='unavailable-pending'")
            .bind(&failure).execute(&mut *transaction).await.unwrap();
        assert_eq!(changed.rows_affected(), 1);
        let rejected = sqlx::query(
            "UPDATE source_resolutions SET failure=NULL WHERE id='unavailable-pending'",
        )
        .execute(&mut *transaction)
        .await
        .unwrap_err();
        assert_eq!(
            rejected.as_database_error().unwrap().message(),
            "Source resolution terminal results are immutable"
        );
        sqlx::query("ROLLBACK TO owned_failure_guard")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("RELEASE owned_failure_guard")
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.rollback().await.unwrap();
}
