#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn migration_0078_only_adds_the_credited_artist_tables_and_changes_no_credit() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("credited-artists-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(77).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);
         INSERT INTO libraries(id,name) VALUES('lib','Lib');
         INSERT INTO library_members(library_id,user_id) VALUES('lib','one');
         INSERT INTO recordings(id,library_id,title,artist) VALUES('rec','lib','T','A & B');
         INSERT INTO account_artists(user_id,id,library_id,name) VALUES('one','artist','lib','A & B');
         INSERT INTO account_recording_credits(user_id,recording_id,artist_id) VALUES('one','rec','artist');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(78).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    for object in &before {
        assert!(after.contains(object), "{object:?} must be kept exactly");
    }
    schema::assert_version(&mut connection, 78).await;
    let credit: (String, String) = sqlx::query_as(
        "SELECT a.name,c.recording_id FROM account_recording_credits c JOIN account_artists a ON a.id=c.artist_id",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(credit, ("A & B".into(), "rec".into()));
    for table in TABLES {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(rows, 0, "Earlier observations carry no structured credit");
    }
    connection.close().await.unwrap();
}

const TABLES: [&str; 3] = [
    "recording_credit_observations",
    "recording_credited_artist_observations",
    "account_credit_keys",
];

#[tokio::test]
async fn the_tables_refuse_malformed_rows_and_changes() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("credited-artists-checks.db"))
        .create_if_missing(true)
        .foreign_keys(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(78).run(&mut connection).await.unwrap();
    for rejected in [
        "INSERT INTO recording_credit_observations(observation_id,recording_id,credit_label,credit_key) VALUES('o','r','','k')",
        "INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role) VALUES('o',16,'r','A','main')",
        "INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role) VALUES('o',0,'r','','main')",
        "INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role) VALUES('o',0,'r','A','producer')",
        "INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role,page_url) VALUES('o',0,'r','A','main','http://x.example/a')",
        "INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role,provider_artist_id) VALUES('o',0,'r','A','main','')",
    ] {
        assert!(
            sqlx::query(rejected)
                .execute(&mut connection)
                .await
                .is_err(),
            "{rejected}"
        );
    }
    sqlx::query("INSERT INTO recording_credit_observations(observation_id,recording_id,credit_label,credit_key) VALUES('o','r','A','a')")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role) VALUES('o',0,'r','A','main')")
        .execute(&mut connection)
        .await
        .unwrap();
    for changed in [
        "UPDATE recording_credit_observations SET credit_label='B'",
        "UPDATE recording_credited_artist_observations SET name='B'",
    ] {
        assert!(
            sqlx::query(changed).execute(&mut connection).await.is_err(),
            "{changed}"
        );
    }
    connection.close().await.unwrap();
}
