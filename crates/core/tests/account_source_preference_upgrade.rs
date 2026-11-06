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
async fn migration_0089_adds_the_preference_table_and_keeps_every_prior_object() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = connect("preference-upgrade.db", &directory).await;
    schema::through(88).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(89).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    for object in &before {
        assert!(after.contains(object), "{object:?} must be kept exactly");
    }
    schema::assert_version(&mut connection, 89).await;
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(accounts, 2);
    let preferences: i64 = sqlx::query_scalar("SELECT count(*) FROM account_source_preferences")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(preferences, 0);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn preference_rows_are_bounded_and_leave_with_their_account() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = connect("preference-checks.db", &directory).await;
    schema::through(89).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    for (revision, fallback, entries) in
        [(0, 1, "[]"), (1, 2, "[]"), (1, 1, "not json"), (1, 1, "{}")]
    {
        assert!(
            sqlx::query("INSERT INTO account_source_preferences(user_id,revision,fallback,entries) VALUES('one',?,?,?)")
                .bind(revision)
                .bind(fallback)
                .bind(entries)
                .execute(&mut connection)
                .await
                .is_err(),
            "{revision} {fallback} {entries}"
        );
    }
    let too_many = format!("[{}]", vec!["1"; 33].join(","));
    assert!(
        sqlx::query(
            "INSERT INTO account_source_preferences(user_id,revision,entries) VALUES('one',1,?)"
        )
        .bind(&too_many)
        .execute(&mut connection)
        .await
        .is_err()
    );
    sqlx::query(
        "INSERT INTO account_source_preferences(user_id,revision,entries) VALUES('one',1,'[]')",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let fallback: i64 = sqlx::query_scalar("SELECT fallback FROM account_source_preferences")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(fallback, 1);
    sqlx::query("DELETE FROM users WHERE id='one'")
        .execute(&mut connection)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM account_source_preferences")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(left, 0);
    connection.close().await.unwrap();
}
