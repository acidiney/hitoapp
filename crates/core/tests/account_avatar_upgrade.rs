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
async fn migration_0087_adds_the_avatar_table_and_keeps_every_prior_object() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = connect("avatar-upgrade.db", &directory).await;
    schema::through(86).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin,display_name) VALUES('one','one','hash',1,'One'),('two','two','hash',0,NULL);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(87).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    for object in &before {
        assert!(after.contains(object), "{object:?} must be kept exactly");
    }
    schema::assert_version(&mut connection, 87).await;
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(accounts, 2);
    let avatars: i64 = sqlx::query_scalar("SELECT count(*) FROM account_avatars")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(avatars, 0);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn avatar_rows_are_bounded_and_leave_with_their_account() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = connect("avatar-checks.db", &directory).await;
    schema::through(87).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let hash = "a".repeat(64);
    assert!(
        sqlx::query(
            "INSERT INTO account_avatars(user_id,content_hash,bytes) VALUES('one','short',x'01')"
        )
        .execute(&mut connection)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("INSERT INTO account_avatars(user_id,content_hash,bytes) VALUES('one',?,zeroblob(262145))")
            .bind(&hash)
            .execute(&mut connection)
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO account_avatars(user_id,content_hash,bytes) VALUES('one',?,x'01')")
        .bind(&hash)
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id='one'")
        .execute(&mut connection)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM account_avatars")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(left, 0);
    connection.close().await.unwrap();
}
