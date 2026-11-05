#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn migration_0084_only_adds_the_connection_table_and_its_indexes() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("plugin-connections-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(83).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);
         INSERT INTO plugin_services(id,name,base_url) VALUES('service','Service','https://provider.test/');
         INSERT INTO plugin_installations(id,user_id,service_id,manifest) VALUES('installation','one','service','{}');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    schema::through(84).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    for object in &before {
        assert!(after.contains(object), "{object:?} must be kept exactly");
    }
    schema::assert_version(&mut connection, 84).await;
    let connections: i64 = sqlx::query_scalar("SELECT count(*) FROM plugin_connections")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        connections, 0,
        "history must not imply a provider connection"
    );
    let installations: i64 = sqlx::query_scalar("SELECT count(*) FROM plugin_installations")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(installations, 1);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn a_connected_row_requires_tokens_and_a_reauthorization_row_cannot_keep_them() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("plugin-connections-checks.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(84).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);
         INSERT INTO plugin_services(id,name,base_url) VALUES('service','Service','https://provider.test/');
         INSERT INTO plugin_installations(id,user_id,service_id,manifest) VALUES('installation','one','service','{}');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let digest = vec![7u8; 32];
    let tokens = vec![1u8; 80];
    for (status, tokens) in [
        ("connected", None::<&Vec<u8>>),
        ("needs_reauthorization", Some(&tokens)),
    ] {
        let rejected = sqlx::query("INSERT INTO plugin_connections(installation_id,user_id,service_id,declaration_digest,status,tokens,scopes) VALUES('installation','one','service',?,?,?,'')")
            .bind(&digest).bind(status).bind(tokens).execute(&mut connection).await;
        assert!(rejected.is_err(), "{status}");
    }
    connection.close().await.unwrap();
}
