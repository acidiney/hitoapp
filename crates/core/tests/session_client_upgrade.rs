#[path = "fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

const LIFETIME: i64 = 2_592_000;

#[tokio::test]
async fn migration_0082_backfills_created_and_last_seen_and_keeps_sessions_valid() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("session-client-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(81).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('legacy-user','legacy','hash',1);
         INSERT INTO sessions(token_hash,user_id,expires_at) VALUES('old','legacy-user',4102444800);
         INSERT INTO sessions(token_hash,user_id,expires_at) VALUES('older','legacy-user',2000000000);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(82).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    assert!(
        before
            .iter()
            .filter(|object| object.1 != "sessions")
            .all(|object| after.contains(object))
    );
    assert_eq!(before.len(), after.len());

    let rows: Vec<(String, i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT token_hash,expires_at,created_at,last_seen_at,client FROM sessions ORDER BY token_hash",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "old".into(),
                4102444800,
                4102444800 - LIFETIME,
                4102444800 - LIFETIME,
                None
            ),
            (
                "older".into(),
                2000000000,
                2000000000 - LIFETIME,
                2000000000 - LIFETIME,
                None
            ),
        ]
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn migrated_sessions_still_authenticate_and_the_client_column_is_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(81).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('legacy-user','legacy','hash',1);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let token = "a".repeat(64);
    let hash = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(token.as_bytes()))
    };
    sqlx::query(
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES(?,'legacy-user',4102444800)",
    )
    .bind(&hash)
    .execute(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();

    let hito = hito_core::open(hito_core::CoreConfig::new(
        database,
        directory.path().join("music"),
        hito_core::HostDeviceId::new("session-client-upgrade").unwrap(),
    ))
    .await
    .unwrap();
    let actor = hito.identity.authenticate(&token).await.unwrap();
    assert_eq!(actor.user().username, "legacy");
    let listed = hito.identity.sessions(&actor).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].created_at, 4102444800 - LIFETIME);
    assert_eq!(listed[0].client, None);
    assert!(listed[0].current);

    let mut connection = SqliteConnection::connect_with(&options.create_if_missing(false))
        .await
        .unwrap();
    for client in ["", &"x".repeat(65)] {
        assert!(
            sqlx::query("UPDATE sessions SET client = ?")
                .bind(client)
                .execute(&mut connection)
                .await
                .is_err()
        );
    }
    sqlx::query("UPDATE sessions SET client = ?")
        .bind("x".repeat(64))
        .execute(&mut connection)
        .await
        .unwrap();
}
