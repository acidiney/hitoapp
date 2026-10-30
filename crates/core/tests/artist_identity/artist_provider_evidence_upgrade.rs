#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

const ISRC: &str = "USRC17607839";

#[tokio::test]
async fn provider_evidence_is_added_without_implying_isrcs_matches_or_checks() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("provider-evidence-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(60).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('legacy','legacy','hash',1);
         INSERT INTO libraries(id,name,owner_id) VALUES('library','Library','legacy');
         INSERT INTO recordings(id,library_id,title,artist,duration_ms) VALUES('recording','library','Title','Artist',1000);
         INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms)
            VALUES('observation','recording','local','legacy_normalized','Title','Artist',1000);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options.clone().create_if_missing(false))
        .await
        .unwrap();
    schema::through(61).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    assert!(before.iter().all(|object| after.contains(object)));
    schema::assert_version(&mut connection, 61).await;

    for statement in [
        format!("INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES('missing','recording','{ISRC}','file_tag')"),
        format!("INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES('observation','missing','{ISRC}','file_tag')"),
        "INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES('observation','recording','usrc17607839','file_tag')".into(),
        "INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES('observation','recording','USRC1760783','file_tag')".into(),
        format!("INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES('observation','recording','{ISRC}','guess')"),
        "INSERT INTO artist_association_provider_matches(association_id,evidence_kind,receipt_order) VALUES('missing','isrc',1)".into(),
    ] {
        assert!(
            sqlx::query(&statement).execute(&mut connection).await.is_err(),
            "{statement} must be rejected"
        );
    }
    sqlx::query(&format!("INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES('observation','recording','{ISRC}','file_tag')"))
        .execute(&mut connection)
        .await
        .unwrap();
    for statement in [
        "UPDATE recording_isrc_observations SET isrc='GBAAA2000001'",
        "DELETE FROM recording_isrc_observations",
    ] {
        assert!(
            sqlx::query(statement)
                .execute(&mut connection)
                .await
                .is_err(),
            "{statement} must be rejected: ISRC observations are immutable"
        );
    }
    connection.close().await.unwrap();
}
