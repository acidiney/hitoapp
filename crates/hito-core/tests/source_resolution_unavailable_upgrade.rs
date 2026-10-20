#[path = "fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;

use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

type SchemaObject = (String, String, String, Option<String>);

#[tokio::test]
async fn unavailable_outcome_upgrade_retains_history_rowids_leases_and_foreign_keys() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("unavailable-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    through(&all, 36).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!("fixtures/source_resolution_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "fixtures/source_resolution_evidence_upgrade.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    through(&all, 54).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!(
        "fixtures/source_resolution_unavailable_upgrade.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    assert_integrity(&mut connection).await;
    let before = snapshot::capture(&mut connection, 54).await;
    let before_schema = schema(&mut connection).await;
    connection.close().await.unwrap();

    let options = options.create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    through(&all, 55).run(&mut connection).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    assert_integrity(&mut connection).await;
    let after_schema = schema(&mut connection).await;
    let expected_schema: Vec<_> = before_schema
        .into_iter()
        .map(|mut object| {
            if object.0 == "table" && object.1 == "source_resolutions" {
                object.3 = object.3.map(|sql| {
                    sql.replace(
                        "'no_suitable_source'",
                        "'no_suitable_source','provider_unavailable'",
                    )
                });
            }
            object
        })
        .collect();
    assert!(
        after_schema == expected_schema,
        "only the declared resolution outcome constraint may change; indexes, foreign keys and triggers must remain exact"
    );
    assert_guards(&mut connection).await;
    snapshot::assert_preserved(&mut connection, &before).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    assert_integrity(&mut connection).await;
    let version: (i64, i64) =
        sqlx::query_as("SELECT max(version),count(*) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert!(
        version == (55, 55),
        "the forward migration must remain durable after reopening"
    );
    connection.close().await.unwrap();
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

async fn schema(connection: &mut SqliteConnection) -> Vec<SchemaObject> {
    sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .fetch_all(connection)
        .await
        .unwrap()
}

async fn assert_integrity(connection: &mut SqliteConnection) {
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let deferred: i64 = sqlx::query_scalar("PRAGMA defer_foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(
        enabled == 1 && deferred == 0,
        "foreign-key enforcement must stay enabled without retained deferred checks"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty(),
        "every retained reference must resolve after the parent table rebuild"
    );
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(connection)
        .await
        .unwrap();
    assert!(
        integrity == ["ok"],
        "the upgraded database must pass integrity checks"
    );
}

async fn assert_guards(connection: &mut SqliteConnection) {
    let mut transaction = connection.begin().await.unwrap();
    for (statement, expected) in [
        (
            "UPDATE source_resolutions SET created_at=created_at+1 WHERE id='unavailable-pending'",
            "Source resolution identity is immutable",
        ),
        (
            "UPDATE source_resolutions SET outcome='provider_unavailable' WHERE id='evidence-resolution'",
            "Source resolution terminal results are immutable",
        ),
        (
            "UPDATE source_resolution_receipts SET request_id='changed' WHERE resolution_id='evidence-resolution'",
            "Source resolution receipts are immutable",
        ),
        (
            "DELETE FROM source_resolution_receipts WHERE resolution_id='evidence-resolution'",
            "Source resolution receipts must be retained",
        ),
        (
            "UPDATE automatic_source_decisions SET musical_metadata=NULL WHERE resolution_id='unavailable-musical-ready'",
            "Automatic source decisions are immutable",
        ),
        (
            "DELETE FROM automatic_source_decisions WHERE resolution_id='unavailable-musical-ready'",
            "Automatic source decisions must be retained",
        ),
    ] {
        let rejected = sqlx::query(statement).execute(&mut *transaction).await;
        assert!(
            rejected.err().and_then(|error| error
                .as_database_error()
                .map(|error| error.message() == expected))
                == Some(true),
            "retained immutable history guards must reject changes"
        );
    }
    let missing_parent = sqlx::query("INSERT INTO source_resolution_receipts(user_id,request_id,recording_id,resolution_id) VALUES ('migration-owner','missing-parent','evidence-recording','absent-resolution')")
        .execute(&mut *transaction).await;
    assert!(
        missing_parent.err().and_then(|error| error
            .as_database_error()
            .map(|error| error.is_foreign_key_violation()))
            == Some(true),
        "retained receipts must still enforce the parent resolution foreign key"
    );
    let duplicate_owner = sqlx::query("INSERT INTO source_resolutions(id,user_id,recording_id,target_digest,choice_revision,lease_token,lease_until) SELECT 'duplicate-pending',user_id,recording_id,target_digest,choice_revision,'other-lease',9999999999 FROM source_resolutions WHERE id='unavailable-pending'")
        .execute(&mut *transaction).await;
    assert!(
        duplicate_owner.err().and_then(|error| error
            .as_database_error()
            .map(|error| error.is_unique_violation()))
            == Some(true),
        "the partial unique index must retain the single pending owner"
    );
    let unknown_outcome = sqlx::query("UPDATE source_resolutions SET outcome='unknown',completed_at=1401,lease_token=NULL,lease_until=NULL WHERE id='unavailable-pending'")
        .execute(&mut *transaction).await;
    assert!(
        unknown_outcome.err().and_then(|error| error
            .as_database_error()
            .map(|error| error.is_check_violation()))
            == Some(true),
        "the rebuilt outcome constraint must continue rejecting unknown results"
    );
    let settled = sqlx::query("UPDATE source_resolutions SET outcome='provider_unavailable',completed_at=1401,lease_token=NULL,lease_until=NULL WHERE id='unavailable-pending' AND lease_token='owned-pending-lease'")
        .execute(&mut *transaction).await.unwrap();
    assert!(
        settled.rows_affected() == 1,
        "the retained exact lease must permit the new typed terminal result"
    );
    let terminal = sqlx::query(
        "UPDATE source_resolutions SET outcome='cancelled' WHERE id='unavailable-pending'",
    )
    .execute(&mut *transaction)
    .await;
    assert!(
        terminal.err().and_then(|error| error
            .as_database_error()
            .map(|error| error.message() == "Source resolution terminal results are immutable"))
            == Some(true),
        "the new terminal outcome must receive the existing immutability guard"
    );
    transaction.rollback().await.unwrap();
}
