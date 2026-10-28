#[path = "fixtures/collection_artwork_schema.rs"]
mod schema;
#[path = "fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;

use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

type SchemaObject = (String, String, String, Option<String>);

#[tokio::test]
async fn album_edition_choices_are_added_without_touching_existing_data() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("album-edition-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
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
    through(&all, 54).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!(
        "fixtures/source_resolution_unavailable_upgrade.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    through(&all, 55).run(&mut connection).await.unwrap();
    assert_integrity(&mut connection).await;
    let before = snapshot::capture(&mut connection, 55).await;
    let before_schema = schema_of(&mut connection).await;
    assert!(
        before_schema
            .iter()
            .all(|object| !object.1.starts_with("album_edition_")),
        "the previous schema has no album edition objects"
    );
    connection.close().await.unwrap();

    let options = options.create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    through(&all, 56).run(&mut connection).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    assert_integrity(&mut connection).await;

    let after_schema = schema_of(&mut connection).await;
    for object in &before_schema {
        assert!(
            after_schema.contains(object),
            "the migration must be additive: every prior schema object stays exact"
        );
    }
    let mut added: Vec<(&str, &str)> = after_schema
        .iter()
        .filter(|object| !before_schema.iter().any(|old| old.1 == object.1))
        .map(|object| (object.0.as_str(), object.1.as_str()))
        .collect();
    let mut declared = schema::album_schema::ADDED.to_vec();
    added.sort_unstable();
    declared.sort_unstable();
    assert_eq!(added, declared);
    schema::assert_version(&mut connection, 56).await;
    guards(&mut connection).await;
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
    assert_eq!(version, (56, 56), "the forward migration stays durable");
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

async fn schema_of(connection: &mut SqliteConnection) -> Vec<SchemaObject> {
    sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .fetch_all(connection)
        .await
        .unwrap()
}

async fn assert_integrity(connection: &mut SqliteConnection) {
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty()
    );
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(connection)
        .await
        .unwrap();
    assert_eq!(integrity, ["ok"]);
}

async fn message(connection: &mut SqliteConnection, statement: &str) -> String {
    sqlx::query(statement)
        .execute(connection)
        .await
        .err()
        .and_then(|error| error.as_database_error().map(|e| e.message().to_owned()))
        .unwrap_or_default()
}

const CHOOSE: &str = "INSERT INTO album_edition_choices(user_id,release_id,revision,recording_id,association_id,provider_kind,plugin_installation_id,release_group_id,edition_id)
    VALUES('migration-owner','migration-release',1,'evidence-recording','album-association','http_plugin','migration-installation','group','edition')";

async fn guards(connection: &mut SqliteConnection) {
    let mut transaction = connection.begin().await.unwrap();
    sqlx::query("DROP TRIGGER recording_artist_association_scope")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("INSERT INTO account_artists(user_id,id,library_id,name) VALUES('migration-owner','album-artist','migration-library','Album artist')")
        .execute(&mut *transaction).await.unwrap();
    sqlx::query("INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,observation_id,role,provider_kind,plugin_installation_id,provider_artist_id,evidence)
        VALUES('album-association','migration-owner','migration-library','evidence-recording','album-artist','evidence-primary-observation','recording_artist','http_plugin','migration-installation','artist','user_confirmed')")
        .execute(&mut *transaction).await.unwrap();

    for partial in [
        "INSERT INTO album_edition_choices(user_id,release_id,revision,recording_id) VALUES('migration-owner','migration-release',1,'evidence-recording')",
        "INSERT INTO album_edition_choices(user_id,release_id,revision) VALUES('migration-owner','migration-release',0)",
    ] {
        assert!(
            message(&mut transaction, partial).await.contains("CHECK"),
            "a link is either complete or cleared"
        );
    }
    let mismatched = CHOOSE.replace("'migration-installation','group'", "NULL,'group'");
    assert!(
        !message(&mut transaction, &mismatched).await.is_empty(),
        "the provider installation must be the association's installation"
    );
    for foreign in [
        CHOOSE.replace("'migration-owner'", "'migration-other'"),
        CHOOSE.replace("'evidence-recording',", "'migration-recording',"),
        CHOOSE.replace("'album-association'", "'absent-association'"),
    ] {
        let rejected = message(&mut transaction, &foreign).await;
        assert!(
            rejected.contains("Album edition choices require") || rejected.contains("FOREIGN KEY"),
            "a choice must use the account's own association: {rejected}"
        );
    }
    let long = CHOOSE.replace("'group'", &format!("'{}'", "g".repeat(257)));
    assert!(message(&mut transaction, &long).await.contains("CHECK"));

    sqlx::query(CHOOSE)
        .execute(&mut *transaction)
        .await
        .unwrap();
    let duplicate = message(&mut transaction, CHOOSE).await;
    assert!(duplicate.contains("UNIQUE"), "one link per account release");
    assert_eq!(
        message(
            &mut transaction,
            "UPDATE album_edition_choices SET release_id='another'"
        )
        .await,
        "Album edition choices require the account's own artist association"
    );
    sqlx::query("UPDATE album_edition_choices SET revision=2,recording_id=NULL,association_id=NULL,provider_kind=NULL,plugin_installation_id=NULL,release_group_id=NULL,edition_id=NULL")
        .execute(&mut *transaction).await.unwrap();

    sqlx::query("INSERT INTO album_edition_choice_receipts(user_id,request_id,request_digest,receipt) VALUES('migration-owner','request','digest','{}')")
        .execute(&mut *transaction).await.unwrap();
    assert_eq!(
        message(
            &mut transaction,
            "UPDATE album_edition_choice_receipts SET request_digest='changed'"
        )
        .await,
        "Album edition receipts are immutable"
    );
    assert_eq!(
        message(
            &mut transaction,
            "DELETE FROM album_edition_choice_receipts"
        )
        .await,
        "Album edition receipts must be retained"
    );
    transaction.rollback().await.unwrap();
}
