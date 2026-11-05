#[path = "fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

async fn connect(name: &str, directory: &tempfile::TempDir) -> SqliteConnection {
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join(name))
        .create_if_missing(true)
        .foreign_keys(true);
    SqliteConnection::connect_with(&options).await.unwrap()
}

#[tokio::test]
async fn migration_0086_adds_a_nullable_display_name_and_keeps_every_account() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = connect("display-name-upgrade.db", &directory).await;
    schema::through(85).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0);
         INSERT INTO sessions(token_hash,user_id,expires_at) VALUES('token','one',4102444800);
         INSERT INTO libraries(id,name,owner_id) VALUES('library','Library','one');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(86).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    assert!(
        before
            .iter()
            .filter(|object| object.1 != "users")
            .all(|object| after.contains(object))
    );
    assert_eq!(before.len(), after.len());
    schema::assert_version(&mut connection, 86).await;

    let rows: Vec<(String, String, bool, Option<String>)> =
        sqlx::query_as("SELECT id,username,admin,display_name FROM users ORDER BY id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            ("one".into(), "one".into(), true, None),
            ("two".into(), "two".into(), false, None),
        ]
    );
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(sessions, 1);
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
async fn the_database_refuses_blank_or_overlong_display_names() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = connect("display-name-checks.db", &directory).await;
    schema::through(86).run(&mut connection).await.unwrap();
    sqlx::raw_sql("INSERT INTO users(id,username,password_hash) VALUES('one','one','hash');")
        .execute(&mut connection)
        .await
        .unwrap();
    for rejected in [String::new(), "a".repeat(65), "🦉".repeat(65)] {
        let result = sqlx::query("UPDATE users SET display_name = ? WHERE id = 'one'")
            .bind(&rejected)
            .execute(&mut connection)
            .await;
        assert!(result.is_err(), "{} characters", rejected.chars().count());
    }
    for accepted in ["a".repeat(64), "🦉".repeat(64)] {
        sqlx::query("UPDATE users SET display_name = ? WHERE id = 'one'")
            .bind(&accepted)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    connection.close().await.unwrap();
}
