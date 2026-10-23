use hito_plugin_sdk::RecordingMetadata;
use sqlx::{
    Connection, SqliteConnection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::{borrow::Cow, collections::BTreeMap};

#[path = "fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;
use snapshot::{assert_preserved, capture};

#[tokio::test]
async fn source_choice_reset_upgrade_preserves_owned_manual_pins_and_completed_history() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("reset.db"))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    through(&all, 36).run(&mut connection).await.unwrap();
    for fixture in [
        include_str!("fixtures/source_resolution_upgrade.sql"),
        include_str!("fixtures/source_resolution_evidence_upgrade.sql"),
    ] {
        sqlx::raw_sql(fixture)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    through(&all, 37).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!("fixtures/manual_source_choice_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    through(&all, 38).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "UPDATE recording_source_choices SET binding_id='evidence-automatic',revision=8 WHERE user_id='migration-owner' AND recording_id='evidence-recording';
         INSERT INTO source_choice_receipts(rowid,user_id,request_id,recording_id,source_id,revision) VALUES (2153,'migration-owner','owned-choice-before-reset','evidence-recording','evidence-source',8);",
    )
    .execute(&mut connection).await.unwrap();
    assert_version(&mut connection, 38).await;
    assert_owned_history(&mut connection).await;
    assert_integrity(&mut connection).await;
    let before = capture(&mut connection, 38).await;
    let before_schema = schema(&mut connection).await;
    let before_authorities = authorities(&mut connection).await;
    assert!(
        before_authorities.len() == 5 && before_authorities.values().all(|rows| !rows.is_empty()),
        "every retained authority and source-choice view must have populated owned rows"
    );
    let reset_tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='source_choice_reset_receipts'",
    )
    .fetch_one(&mut connection).await.unwrap();
    assert!(
        reset_tables == 0,
        "the fixture must start before reset receipts exist"
    );
    connection.close().await.unwrap();

    let options = options.create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_preserved(&mut connection, &before).await;
    assert_owned_history(&mut connection).await;
    assert_integrity(&mut connection).await;
    through(&all, 39).run(&mut connection).await.unwrap();
    assert_version(&mut connection, 39).await;
    assert_preserved(&mut connection, &before).await;
    assert_schema(&mut connection, &before_schema).await;
    assert!(
        authorities(&mut connection).await == before_authorities,
        "adding reset receipts must preserve every existing authority and source-choice row"
    );
    assert_no_invented_resets(&mut connection).await;
    assert_guards(&mut connection).await;
    assert_preserved(&mut connection, &before).await;
    assert_no_invented_resets(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_version(&mut connection, 39).await;
    assert_preserved(&mut connection, &before).await;
    assert_schema(&mut connection, &before_schema).await;
    assert!(
        authorities(&mut connection).await == before_authorities,
        "retained authority and source-choice rows must survive a fresh connection"
    );
    assert_owned_history(&mut connection).await;
    assert_no_invented_resets(&mut connection).await;
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
        "apply the intended production migrations"
    );
}

async fn assert_owned_history(connection: &mut SqliteConnection) {
    let pin: (String, i64, String, i64) = sqlx::query_as(
        "SELECT choice.binding_id,choice.revision,receipt.source_id,receipt.revision
         FROM recording_source_choices choice JOIN source_choice_receipts receipt ON receipt.user_id=choice.user_id AND receipt.recording_id=choice.recording_id
         WHERE choice.user_id='migration-owner' AND choice.recording_id='evidence-recording' AND receipt.request_id='owned-choice-before-reset'",
    )
    .fetch_one(&mut *connection).await.unwrap();
    assert!(
        pin == ("evidence-automatic".into(), 8, "evidence-source".into(), 8),
        "retain the existing owned manual pin and its immutable choose receipt"
    );
    let decisions: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT policy_version,musical_metadata FROM automatic_source_decisions ORDER BY resolution_id",
    )
    .fetch_all(&mut *connection).await.unwrap();
    assert!(
        decisions.len() == 2
            && decisions[0].0 == "hito.musical-source.v1"
            && decisions[0].1.is_none()
            && decisions[1].0 == "hito.musical-source.metadata.v1",
        "retain completed legacy and enriched automatic history"
    );
    let metadata: RecordingMetadata = serde_json::from_str(decisions[1].1.as_ref().unwrap())
        .unwrap_or_else(|_| panic!("the owned retained musical metadata must decode"));
    assert!(
        metadata.validate().is_ok(),
        "the retained musical metadata must satisfy its bounded SDK contract"
    );
    let private: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT liked FROM likes WHERE user_id='migration-owner' AND recording_id='migration-recording'),
            (SELECT position_ms FROM listening_progress WHERE user_id='migration-owner' AND recording_id='migration-recording'),
            (SELECT count(*) FROM personal_recording_metadata WHERE user_id='migration-owner' AND recording_id='migration-recording'),
            (SELECT count(*) FROM playlist_entries WHERE playlist_id='migration-playlist' AND recording_id='migration-recording')",
    )
    .fetch_one(connection).await.unwrap();
    assert!(
        private == (1, 6731, 1, 2),
        "the owned fixture must retain private state and distinct duplicate playlist entries"
    );
}

async fn schema(connection: &mut SqliteConnection) -> BTreeMap<String, String> {
    sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE type IN ('table','trigger','view','index') AND sql IS NOT NULL ORDER BY name")
        .fetch_all(connection).await.unwrap().into_iter().collect()
}

async fn assert_schema(connection: &mut SqliteConnection, before: &BTreeMap<String, String>) {
    let after = schema(connection).await;
    assert!(
        before
            .iter()
            .all(|(name, definition)| after.get(name) == Some(definition)),
        "adding reset receipts must retain every prior schema definition exactly"
    );
}

async fn authorities(connection: &mut SqliteConnection) -> BTreeMap<String, Vec<String>> {
    let mut result = BTreeMap::new();
    for (name, statement) in [
        (
            "current_artwork_fetch_authority",
            "SELECT id FROM current_artwork_fetch_authority ORDER BY id",
        ),
        (
            "current_download_authority",
            "SELECT id FROM current_download_authority ORDER BY id",
        ),
        (
            "current_source_preparation_authority",
            "SELECT id FROM current_source_preparation_authority ORDER BY id",
        ),
        (
            "source_choice_bindings",
            "SELECT json_array(user_id,recording_id,binding_id) FROM source_choice_bindings ORDER BY user_id,recording_id,binding_id",
        ),
        (
            "download_file_origins",
            "SELECT json_array(file_id,account_id,host_device,recording_id,binding_id) FROM download_file_origins ORDER BY file_id",
        ),
    ] {
        let rows = sqlx::query_scalar(statement)
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        result.insert(name.to_owned(), rows);
    }
    result
}

async fn assert_no_invented_resets(connection: &mut SqliteConnection) {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM source_choice_reset_receipts")
        .fetch_one(connection)
        .await
        .unwrap();
    assert!(
        count == 0,
        "migration must not invent reset receipts or reset an existing manual pin"
    );
}

async fn assert_guards(connection: &mut SqliteConnection) {
    let mut transaction = connection.begin().await.unwrap();
    for (request, expected, revision) in [
        ("owned-reset-guard", 8_i64, 9_i64),
        ("owned-zero-reset-guard", 0, 1),
    ] {
        sqlx::query("INSERT INTO source_choice_reset_receipts(user_id,request_id,recording_id,expected_revision,revision) VALUES ('migration-owner',?,'evidence-recording',?,?)")
            .bind(request).bind(expected).bind(revision)
            .execute(&mut *transaction).await.unwrap();
    }
    for (statement, message) in [
        (
            "UPDATE source_choice_reset_receipts SET revision=10 WHERE request_id='owned-reset-guard'",
            "Source choice reset receipts are immutable",
        ),
        (
            "DELETE FROM source_choice_reset_receipts WHERE request_id='owned-reset-guard'",
            "Source choice reset receipts must be retained",
        ),
        (
            "UPDATE source_choice_receipts SET revision=9 WHERE request_id='owned-choice-before-reset'",
            "Source choice receipts are immutable",
        ),
        (
            "DELETE FROM source_choice_receipts WHERE request_id='owned-choice-before-reset'",
            "Source choice receipts must be retained",
        ),
        (
            "UPDATE recording_source_choices SET binding_id='migration-manual' WHERE user_id='migration-owner' AND recording_id='evidence-recording'",
            "Source choice binding ownership does not match",
        ),
        (
            "UPDATE automatic_source_decisions SET musical_metadata='{}' WHERE resolution_id='musical-resolution'",
            "Automatic source decisions are immutable",
        ),
    ] {
        let result = sqlx::query(statement).execute(&mut *transaction).await;
        assert!(
            result.err().and_then(|error| error
                .as_database_error()
                .map(|error| error.message() == message))
                == Some(true),
            "new reset receipts and retained choice history must reject mutation"
        );
    }
    for (expected, revision) in [(-1_i64, 0_i64), (0, 0), (8, 8), (8, 10)] {
        let result = sqlx::query("INSERT INTO source_choice_reset_receipts(user_id,request_id,recording_id,expected_revision,revision) VALUES ('migration-owner','owned-invalid-revision','evidence-recording',?,?)")
            .bind(expected).bind(revision).execute(&mut *transaction).await;
        assert!(
            result.err().and_then(|error| error
                .as_database_error()
                .map(|error| error.is_check_violation()))
                == Some(true),
            "reset receipts must reject negative expectations, nonpositive revisions and revisions outside the next step"
        );
    }
    for statement in [
        "INSERT INTO source_choice_reset_receipts(user_id,request_id,recording_id,expected_revision,revision) VALUES ('reset-missing-user','owned-invalid-user','evidence-recording',0,1)",
        "INSERT INTO source_choice_reset_receipts(user_id,request_id,recording_id,expected_revision,revision) VALUES ('migration-owner','owned-invalid-recording','reset-missing-recording',0,1)",
    ] {
        let result = sqlx::query(statement).execute(&mut *transaction).await;
        assert!(
            result.err().and_then(|error| error
                .as_database_error()
                .map(|error| error.is_foreign_key_violation()))
                == Some(true),
            "new reset receipt foreign keys must reject dangling users and recordings"
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
