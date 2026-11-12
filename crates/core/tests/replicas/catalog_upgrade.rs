use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;
#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;

#[tokio::test]
async fn migration_94_preserves_all_prior_rows_receipts_and_outbox_identities() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("upgrade.db");
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    schema::through(35).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!("../fixtures/source_resolution_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    schema::through(93).run(&mut connection).await.unwrap();
    sqlx::raw_sql("INSERT INTO replica_outbox(ordinal,operation_id,playlist_id,payload,acknowledged,echoed) VALUES(17,'exact-operation','exact-playlist','{\"owned\":true}',1,0); INSERT INTO replica_received_events(sequence,operation_id,client_id,payload) VALUES(23,'exact-event','exact-client','{\"owned\":true}');").execute(&mut connection).await.unwrap();
    let before = snapshot::capture(&mut connection, 93).await;
    schema::through(94).run(&mut connection).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before).await;
    let mut fresh = schema::reference_94().await;
    assert_eq!(
        schema::schema(&mut connection).await,
        schema::schema(&mut fresh).await
    );
    fresh.close().await.unwrap();
    schema::replica_catalog_schema::assert_empty(&mut connection).await;
    connection.close().await.unwrap();
    let mut reopened = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    schema::through(94).run(&mut reopened).await.unwrap();
    snapshot::assert_preserved(&mut reopened, &before).await;
    let violations: Vec<String> =
        sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check")
            .fetch_all(&mut reopened)
            .await
            .unwrap();
    assert!(violations.is_empty());
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut reopened)
            .await
            .unwrap(),
        "ok"
    );
    reopened.close().await.unwrap();
}
