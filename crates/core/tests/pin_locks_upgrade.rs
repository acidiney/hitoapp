#[path = "fixtures/collection_artwork_schema.rs"]
mod schema;

use schema::pin_schema;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn pin_locks_are_added_without_touching_existing_accounts_or_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("pin-locks-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(57).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('legacy-user','legacy','hash',1);
         INSERT INTO sessions(token_hash,user_id,expires_at) VALUES('legacy-session','legacy-user',4102444800);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options.create_if_missing(false))
        .await
        .unwrap();
    schema::through(58).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    assert!(before.iter().all(|object| after.contains(object)));
    let users: (String, String, bool) = sqlx::query_as("SELECT id,username,admin FROM users")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(users, ("legacy-user".into(), "legacy".into(), true));
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(sessions, 1);
    schema::assert_current(&mut connection).await;
    pin_schema::assert_empty(&mut connection).await;

    for invalid in [
        "INSERT INTO account_pin_locks(user_id,pin_hash,idle_minutes) VALUES('legacy-user','h',7)",
        "INSERT INTO account_pin_locks(user_id,pin_hash) VALUES('absent','h')",
        "INSERT INTO session_locks(token_hash,locked) VALUES('legacy-session',2)",
        "INSERT INTO session_locks(token_hash) VALUES('absent-session')",
    ] {
        assert!(sqlx::query(invalid).execute(&mut connection).await.is_err());
    }
    sqlx::query("INSERT INTO account_pin_locks(user_id,pin_hash) VALUES('legacy-user','h')")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO session_locks(token_hash) VALUES('legacy-session')")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sessions WHERE token_hash='legacy-session'")
        .execute(&mut connection)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM session_locks")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(remaining, 0, "ending a session removes its lock state");
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .unwrap()
            .is_empty()
    );
}
