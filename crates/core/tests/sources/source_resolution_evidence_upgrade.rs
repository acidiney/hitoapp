use sqlx::{
    Connection, SqliteConnection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::borrow::Cow;

#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;
use snapshot::{assert_preserved, capture};

#[tokio::test]
async fn musical_evidence_upgrade_preserves_populated_legacy_decisions_and_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("evidence.db"))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    through(&all, 36).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!("../fixtures/source_resolution_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../fixtures/source_resolution_evidence_upgrade.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    assert_version(&mut connection, 36).await;
    assert_legacy_chain(&mut connection).await;
    assert_integrity(&mut connection).await;
    let before = capture(&mut connection, 36).await;
    let metadata_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pragma_table_info('automatic_source_decisions') WHERE name='musical_metadata'",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert!(
        metadata_columns == 0,
        "the fixture must start on the legacy schema"
    );
    connection.close().await.unwrap();

    let options = options.create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_preserved(&mut connection, &before).await;
    assert_integrity(&mut connection).await;
    through(&all, 37).run(&mut connection).await.unwrap();
    assert_version(&mut connection, 37).await;
    assert_preserved(&mut connection, &before).await;
    assert_legacy_metadata_is_null(&mut connection).await;
    assert_guards(&mut connection).await;
    assert_preserved(&mut connection, &before).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_version(&mut connection, 37).await;
    assert_preserved(&mut connection, &before).await;
    assert_legacy_chain(&mut connection).await;
    assert_legacy_metadata_is_null(&mut connection).await;
    assert_integrity(&mut connection).await;
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

async fn assert_version(connection: &mut SqliteConnection, expected: i64) {
    let version: (i64, i64) =
        sqlx::query_as("SELECT max(version),count(*) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(connection)
            .await
            .unwrap();
    assert!(
        version == (expected, expected),
        "the intended production migrations must be applied"
    );
}

async fn assert_legacy_chain(connection: &mut SqliteConnection) {
    let chain: (String, String, String, i64, i64, i64) = sqlx::query_as(
        "SELECT decision.policy_version,binding.evidence,resolution.outcome,choice.revision,
            decision.installation_revision,decision.service_revision
         FROM automatic_source_decisions decision
         JOIN provider_items binding ON binding.id=decision.binding_id
         JOIN source_resolutions resolution ON resolution.id=decision.resolution_id
         JOIN recording_source_choices choice ON choice.user_id=resolution.user_id AND choice.recording_id=resolution.recording_id
         JOIN source_resolution_receipts receipt ON receipt.resolution_id=resolution.id AND receipt.user_id=resolution.user_id AND receipt.recording_id=resolution.recording_id
         WHERE resolution.id='evidence-resolution'",
    )
    .fetch_one(connection)
    .await
    .unwrap();
    assert!(
        chain
            == (
                "hito.musical-source.v1".into(),
                "automatic_policy".into(),
                "ready".into(),
                7,
                1,
                1,
            ),
        "the owned fixture must retain its completed legacy automatic decision and receipt"
    );
}

async fn assert_legacy_metadata_is_null(connection: &mut SqliteConnection) {
    let metadata: Option<String> = sqlx::query_scalar(
        "SELECT musical_metadata FROM automatic_source_decisions WHERE resolution_id='evidence-resolution'",
    )
    .fetch_one(connection)
    .await
    .unwrap();
    assert!(
        metadata.is_none(),
        "legacy decisions must not invent musical metadata"
    );
}

async fn assert_guards(connection: &mut SqliteConnection) {
    let mut transaction = connection.begin().await.unwrap();
    for (statement, message) in [
        (
            "UPDATE automatic_source_decisions SET candidate_digest=target_digest WHERE resolution_id='evidence-resolution'",
            "Automatic source decisions are immutable",
        ),
        (
            "UPDATE automatic_source_decisions SET musical_metadata='{}' WHERE resolution_id='evidence-resolution'",
            "Automatic source decisions are immutable",
        ),
        (
            "DELETE FROM automatic_source_decisions WHERE resolution_id='evidence-resolution'",
            "Automatic source decisions must be retained",
        ),
        (
            "UPDATE source_resolution_receipts SET request_id='evidence-changed' WHERE resolution_id='evidence-resolution'",
            "Source resolution receipts are immutable",
        ),
        (
            "DELETE FROM source_resolution_receipts WHERE resolution_id='evidence-resolution'",
            "Source resolution receipts must be retained",
        ),
        (
            "UPDATE source_resolutions SET outcome='cancelled' WHERE id='evidence-resolution'",
            "Source resolution terminal results are immutable",
        ),
        (
            "UPDATE provider_items SET evidence='user_confirmed' WHERE id='evidence-automatic'",
            "Provider binding identity is immutable",
        ),
        (
            "DELETE FROM provider_items WHERE id='evidence-automatic'",
            "Provider binding history must be retained",
        ),
        (
            "UPDATE recordings SET metadata_binding_id='evidence-automatic' WHERE id='evidence-recording'",
            "Provider metadata requires its primary binding",
        ),
        (
            "INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence) VALUES ('evidence-wrong-owner','migration-installation','owned-evidence-wrong-owner','migration-other-recording','source','automatic_policy')",
            "Provider recordings require their account library",
        ),
    ] {
        let result = sqlx::query(statement).execute(&mut *transaction).await;
        assert!(
            result.err().and_then(|error| {
                error
                    .as_database_error()
                    .map(|error| error.message() == message)
            }) == Some(true),
            "retained immutable history guards must reject mutation"
        );
    }
    sqlx::query("INSERT INTO source_resolutions(id,user_id,recording_id,target_digest,choice_revision,outcome,created_at,completed_at) SELECT 'evidence-fk-resolution',user_id,recording_id,target_digest,choice_revision,outcome,created_at,completed_at FROM source_resolutions WHERE id='evidence-resolution'")
        .execute(&mut *transaction).await.unwrap();
    for statement in [
        "INSERT INTO source_resolution_receipts(user_id,request_id,recording_id,resolution_id,created_at) VALUES ('migration-owner','evidence-invalid-request','evidence-recording','evidence-missing-resolution',1230)",
        "INSERT INTO automatic_source_decisions(resolution_id,binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,created_at) SELECT 'evidence-missing-resolution',binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,created_at FROM automatic_source_decisions WHERE resolution_id='evidence-resolution'",
        "INSERT INTO automatic_source_decisions(resolution_id,binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,created_at) SELECT 'evidence-fk-resolution','evidence-missing-binding',policy_version,target_digest,candidate_digest,installation_revision,service_revision,created_at FROM automatic_source_decisions WHERE resolution_id='evidence-resolution'",
        "INSERT INTO recording_source_choices(user_id,recording_id,revision) VALUES ('evidence-missing-owner','evidence-recording',7)",
    ] {
        let result = sqlx::query(statement).execute(&mut *transaction).await;
        assert!(
            result.err().and_then(|error| {
                error
                    .as_database_error()
                    .map(|error| error.is_foreign_key_violation())
            }) == Some(true),
            "retained foreign keys must reject dangling resolution history"
        );
    }
    transaction.rollback().await.unwrap();
}

async fn assert_integrity(connection: &mut SqliteConnection) {
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(enabled == 1, "foreign-key enforcement must remain enabled");
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty(),
        "persisted migration rows must satisfy all foreign keys"
    );
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(connection)
        .await
        .unwrap();
    assert!(
        integrity.len() == 1 && integrity[0] == "ok",
        "database integrity must hold"
    );
}
