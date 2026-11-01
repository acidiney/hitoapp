#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn migration_0077_only_adds_the_recording_group_tables_triggers_and_identifier_indexes() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("recording-groups-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(76).run(&mut connection).await.unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(77).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    for object in &before {
        assert!(after.contains(object), "{object:?} must be kept exactly");
    }
    schema::assert_version(&mut connection, 77).await;
    connection.close().await.unwrap();
}

#[tokio::test]
async fn accounts_with_identified_recordings_walk_again_to_group_them() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("recording-groups-requeue.db"))
        .create_if_missing(true)
        .foreign_keys(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(76).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0),('three','three','hash',0);
         INSERT INTO libraries(id,name) VALUES('lib-one','One'),('lib-two','Two'),('lib-three','Three');
         INSERT INTO library_members(library_id,user_id) VALUES('lib-one','one'),('lib-two','two'),('lib-three','three');
         INSERT INTO recordings(id,library_id,title,artist,metadata_observation_id) VALUES
            ('tagged','lib-one','T','A','obs-tagged'),('plain','lib-two','P','A','obs-plain'),('coded','lib-three','C','A','obs-coded');
         INSERT INTO recording_musicbrainz_observations(observation_id,entity,mbid,recording_id)
            VALUES('obs-tagged','recording','0f700000-0000-4000-8000-0000000000a1','tagged');
         INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin)
            VALUES('obs-coded','coded','GBAAA2600001','file_tag');
         INSERT INTO artist_matching_jobs(user_id) VALUES('one');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(77).run(&mut connection).await.unwrap();
    let jobs: Vec<(String, i64)> =
        sqlx::query_as("SELECT user_id,rerun FROM artist_matching_jobs ORDER BY 1")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(jobs, vec![("one".into(), 1), ("three".into(), 0)]);
    connection.close().await.unwrap();
}
