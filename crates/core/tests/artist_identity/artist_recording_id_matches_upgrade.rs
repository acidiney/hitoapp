#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn migration_0076_only_adds_the_recording_identifier_provenance_table_and_its_triggers() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("recording-id-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(75).run(&mut connection).await.unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(76).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    for object in &before {
        assert!(after.contains(object), "{object:?} must be kept exactly");
    }
    schema::assert_version(&mut connection, 76).await;
    connection.close().await.unwrap();
}
