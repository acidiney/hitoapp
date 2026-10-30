use hito_plugin_sdk::RecordingMetadata;
use sqlx::{
    Connection, SqliteConnection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::{borrow::Cow, collections::BTreeMap};

#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;
use snapshot::{assert_preserved, capture};

#[tokio::test]
async fn manual_choice_upgrade_preserves_populated_legacy_and_musical_decisions_without_pinning() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("choice.db"))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    through(&all, 36).run(&mut connection).await.unwrap();
    for fixture in [
        include_str!("../fixtures/source_resolution_upgrade.sql"),
        include_str!("../fixtures/source_resolution_evidence_upgrade.sql"),
    ] {
        sqlx::raw_sql(fixture)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    through(&all, 37).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!("../fixtures/manual_source_choice_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    assert_version(&mut connection, 37).await;
    assert_decisions(&mut connection).await;
    assert_integrity(&mut connection).await;
    let before = capture(&mut connection, 37).await;
    let before_schema = retained_schema(&mut connection).await;
    let before_authorities = authorities(&mut connection).await;
    assert!(
        before_authorities.values().all(|rows| rows.len() == 1),
        "each retained authority view must have a live owned fixture"
    );
    let pin_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pragma_table_info('recording_source_choices') WHERE name='binding_id'",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert!(
        pin_columns == 0,
        "the fixture must start without manual pins"
    );
    connection.close().await.unwrap();

    let options = options.create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_preserved(&mut connection, &before).await;
    assert_decisions(&mut connection).await;
    assert_integrity(&mut connection).await;
    through(&all, 38).run(&mut connection).await.unwrap();
    assert_version(&mut connection, 38).await;
    assert_preserved(&mut connection, &before).await;
    assert_schema(&mut connection, &before_schema).await;
    assert!(
        authorities(&mut connection).await == before_authorities,
        "without a manual pin, retained authority rows must remain unchanged"
    );
    assert_no_invented_pins(&mut connection).await;
    assert_guards(&mut connection).await;
    assert_preserved(&mut connection, &before).await;
    assert_no_invented_pins(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_version(&mut connection, 38).await;
    assert_preserved(&mut connection, &before).await;
    assert_schema(&mut connection, &before_schema).await;
    assert!(
        authorities(&mut connection).await == before_authorities,
        "retained authority rows must survive a fresh connection"
    );
    assert_decisions(&mut connection).await;
    assert_no_invented_pins(&mut connection).await;
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

async fn assert_decisions(connection: &mut SqliteConnection) {
    let decisions: Vec<(String, String, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT decision.resolution_id,decision.policy_version,decision.musical_metadata,binding.evidence,choice.revision
         FROM automatic_source_decisions decision
         JOIN provider_items binding ON binding.id=decision.binding_id
         JOIN source_resolutions resolution ON resolution.id=decision.resolution_id
         JOIN recording_source_choices choice ON choice.user_id=resolution.user_id AND choice.recording_id=resolution.recording_id
         JOIN source_resolution_receipts receipt ON receipt.resolution_id=resolution.id AND receipt.user_id=resolution.user_id AND receipt.recording_id=resolution.recording_id
         WHERE resolution.recording_id='evidence-recording' ORDER BY decision.resolution_id",
    )
    .fetch_all(connection)
    .await
    .unwrap();
    assert!(
        decisions.len() == 2
            && decisions[0].0 == "evidence-resolution"
            && decisions[0].1 == "hito.musical-source.v1"
            && decisions[0].2.is_none()
            && decisions[1].0 == "musical-resolution"
            && decisions[1].1 == "hito.musical-source.metadata.v1"
            && decisions
                .iter()
                .all(|row| row.3 == "automatic_policy" && row.4 == 7),
        "the fixture must retain both completed policy generations with their receipts"
    );
    let metadata = decisions[1].2.as_ref().unwrap();
    let decoded: RecordingMetadata = serde_json::from_str(metadata)
        .unwrap_or_else(|_| panic!("the retained owned musical metadata must decode"));
    assert!(
        decoded.validate().is_ok()
            && decoded.recording.id == "owned-evidence-automatic"
            && decoded.musical_title.as_deref() == Some("Owned evidence track")
            && decoded.source_ids.as_deref()
                == Some(["owned-evidence-source".to_owned()].as_slice()),
        "the enriched fixture must retain bounded musical metadata and its source witness"
    );
}

async fn retained_schema(connection: &mut SqliteConnection) -> BTreeMap<String, String> {
    sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE type IN ('table','trigger','view','index') AND sql IS NOT NULL ORDER BY name")
        .fetch_all(connection).await.unwrap().into_iter().collect()
}

async fn assert_schema(connection: &mut SqliteConnection, before: &BTreeMap<String, String>) {
    let after = retained_schema(connection).await;
    let migration = include_str!("../../migrations/0038_manual_source_choice.sql");
    let start = migration
        .find("CREATE VIEW current_download_authority AS")
        .unwrap();
    let authority = migration[start..].split(';').next().unwrap().trim();
    for (name, definition) in before {
        let expected = match name.as_str() {
            "current_download_authority" => authority.to_owned(),
            "recording_source_choices" => {
                let added = migration
                    .strip_prefix("ALTER TABLE recording_source_choices ADD COLUMN ")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap();
                let mut expected = definition.clone();
                let insertion = expected
                    .find(",\n    PRIMARY KEY(user_id,recording_id)")
                    .expect("the retained choice schema must keep its table constraint");
                expected.insert_str(insertion, &format!(", {added}"));
                expected
            }
            _ => definition.clone(),
        };
        assert!(
            after.get(name) == Some(&expected),
            "retain every prior schema definition except the exact declared authority replacement and nullable pin addition"
        );
    }
}

async fn authorities(connection: &mut SqliteConnection) -> BTreeMap<String, Vec<String>> {
    let mut result = BTreeMap::new();
    for name in [
        "current_artwork_fetch_authority",
        "current_download_authority",
        "current_source_preparation_authority",
    ] {
        let rows: Vec<String> = sqlx::query_scalar(&format!("SELECT id FROM {name} ORDER BY id"))
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        result.insert(name.to_owned(), rows);
    }
    result
}

async fn assert_no_invented_pins(connection: &mut SqliteConnection) {
    let column: (String, i64, Option<String>) = sqlx::query_as(
        "SELECT type,\"notnull\",dflt_value FROM pragma_table_info('recording_source_choices') WHERE name='binding_id'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM recording_source_choices WHERE binding_id IS NOT NULL),(SELECT count(*) FROM source_choice_receipts)",
    )
    .fetch_one(connection)
    .await
    .unwrap();
    assert!(
        column == ("TEXT".to_owned(), 0, None) && counts == (0, 0),
        "the additive nullable binding must not invent manual choices or receipts"
    );
}

async fn assert_guards(connection: &mut SqliteConnection) {
    let mut transaction = connection.begin().await.unwrap();
    sqlx::query("UPDATE recording_source_choices SET binding_id='evidence-automatic',revision=revision+1 WHERE user_id='migration-owner' AND recording_id='evidence-recording'")
        .execute(&mut *transaction).await.unwrap();
    let permitted: Vec<String> = sqlx::query_scalar("SELECT binding_id FROM source_choice_bindings WHERE user_id='migration-owner' AND recording_id='evidence-recording'")
        .fetch_all(&mut *transaction).await.unwrap();
    assert!(
        permitted == ["evidence-automatic"],
        "an owned manual pin must admit only its binding"
    );
    sqlx::query("INSERT INTO source_choice_receipts(user_id,request_id,recording_id,source_id,revision) VALUES ('migration-owner','owned-manual-choice','evidence-recording','evidence-source',8)")
        .execute(&mut *transaction).await.unwrap();
    for (statement, message) in [
        (
            "UPDATE recording_source_choices SET binding_id='migration-manual' WHERE user_id='migration-owner' AND recording_id='evidence-recording'",
            "Source choice binding ownership does not match",
        ),
        (
            "UPDATE recording_source_choices SET user_id='migration-other' WHERE user_id='migration-owner' AND recording_id='evidence-recording'",
            "Source choice binding ownership does not match",
        ),
        (
            "INSERT INTO recording_source_choices(user_id,recording_id,revision,binding_id) VALUES ('migration-other','evidence-recording',1,'evidence-automatic')",
            "Source choice binding ownership does not match",
        ),
        (
            "UPDATE source_choice_receipts SET revision=9 WHERE request_id='owned-manual-choice'",
            "Source choice receipts are immutable",
        ),
        (
            "DELETE FROM source_choice_receipts WHERE request_id='owned-manual-choice'",
            "Source choice receipts must be retained",
        ),
        (
            "UPDATE automatic_source_decisions SET musical_metadata='{}' WHERE resolution_id='musical-resolution'",
            "Automatic source decisions are immutable",
        ),
        (
            "DELETE FROM automatic_source_decisions WHERE resolution_id='musical-resolution'",
            "Automatic source decisions must be retained",
        ),
    ] {
        let result = sqlx::query(statement).execute(&mut *transaction).await;
        assert!(
            result.err().and_then(|error| error
                .as_database_error()
                .map(|error| error.message() == message))
                == Some(true),
            "choice ownership and immutable history guards must reject mutation"
        );
    }
    for statement in [
        "INSERT INTO source_choice_receipts(user_id,request_id,recording_id,source_id,revision) VALUES ('choice-missing-user','owned-invalid-user','evidence-recording','evidence-source',8)",
        "INSERT INTO source_choice_receipts(user_id,request_id,recording_id,source_id,revision) VALUES ('migration-owner','owned-invalid-recording','choice-missing-recording','evidence-source',8)",
        "INSERT INTO recording_source_choices(user_id,recording_id,revision) VALUES ('choice-missing-user','evidence-recording',1)",
    ] {
        let result = sqlx::query(statement).execute(&mut *transaction).await;
        assert!(
            result.err().and_then(|error| error
                .as_database_error()
                .map(|error| error.is_foreign_key_violation()))
                == Some(true),
            "manual choice foreign keys must reject dangling ownership or recording references"
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
