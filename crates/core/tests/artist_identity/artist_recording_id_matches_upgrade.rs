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

#[tokio::test]
async fn recordings_with_a_tagged_recording_id_are_examined_again_and_their_accounts_walk_again() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("recording-id-requeue.db"))
        .create_if_missing(true)
        .foreign_keys(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(75).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0);
         INSERT INTO libraries(id,name) VALUES('lib','Library');
         INSERT INTO library_members(library_id,user_id) VALUES('lib','one'),('lib','two');
         INSERT INTO recordings(id,library_id,title,artist,metadata_observation_id) VALUES
            ('tagged','lib','T','A','obs-tagged'),('plain','lib','P','A','obs-plain');
         INSERT INTO recording_musicbrainz_observations(observation_id,entity,mbid,recording_id)
            VALUES('obs-tagged','recording','0f700000-0000-4000-8000-0000000000a1','tagged');
         INSERT INTO artist_provider_checks(user_id,recording_id,observation_id) VALUES
            ('one','tagged','obs-tagged'),('one','plain','obs-plain');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(76).run(&mut connection).await.unwrap();
    let checks: Vec<String> =
        sqlx::query_scalar("SELECT recording_id FROM artist_provider_checks ORDER BY 1")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(checks, vec!["plain".to_string()]);
    let jobs: Vec<(String, i64)> =
        sqlx::query_as("SELECT user_id,rerun FROM artist_matching_jobs ORDER BY 1")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(jobs, vec![("one".into(), 0), ("two".into(), 0)]);
    connection.close().await.unwrap();
}
