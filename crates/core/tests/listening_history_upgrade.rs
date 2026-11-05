#[path = "fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn migration_0085_adds_history_tables_and_keeps_every_prior_object() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("listening-history-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(84).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0);
         INSERT INTO libraries(id,name,owner_id) VALUES('library','Library','one');
         INSERT INTO recordings(id,library_id,title,artist,duration_ms) VALUES('first','library','First','Artist',1000),('second','library','Second','Artist',1000);
         INSERT INTO listening_progress(user_id,recording_id,position_ms,speed,updated_at) VALUES('one','first',10,1,100),('one','second',20,1.5,200),('two','first',30,1,300);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(85).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    for object in &before {
        assert!(after.contains(object), "{object:?} must be kept exactly");
    }
    schema::assert_version(&mut connection, 85).await;

    let history: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT user_id,recording_id,played_at FROM listening_history ORDER BY user_id,recording_id",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        history,
        vec![
            ("one".into(), "first".into(), 100),
            ("one".into(), "second".into(), 200),
            ("two".into(), "first".into(), 300),
        ]
    );
    let progress: i64 = sqlx::query_scalar("SELECT count(*) FROM listening_progress")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(progress, 3);
    let preferences: i64 = sqlx::query_scalar("SELECT count(*) FROM account_history_preferences")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(preferences, 0, "upgrading must keep recording history on");
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .unwrap()
            .is_empty()
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn history_preference_only_accepts_zero_or_one() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("history-preference-checks.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(85).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO account_history_preferences(user_id,history_enabled) VALUES('one',2)"
        )
        .execute(&mut connection)
        .await
        .is_err()
    );
    sqlx::query("INSERT INTO account_history_preferences(user_id,history_enabled) VALUES('one',0)")
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
}
