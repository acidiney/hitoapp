use super::{connect, through};
use sqlx::Connection;

#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;
#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;

#[tokio::test]
async fn upgrade_92_preserves_every_prior_row_receipt_and_schema_object_except_two_triggers() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned.db");
    let mut connection = connect(&path, 35).await;
    sqlx::raw_sql(include_str!("../fixtures/source_resolution_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    through(92).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO replica_outbox(ordinal,operation_id,playlist_id,payload,acknowledged,echoed)
        VALUES (17,'owned-operation','migration-playlist','{\"owned\":true}',1,1);
        INSERT INTO replica_received_events(sequence,operation_id,client_id,payload)
        VALUES (23,'owned-received','owned-client','{\"owned\":true}');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    integrity(&mut connection).await;
    let before_rows = snapshot::capture(&mut connection, 92).await;
    let before_schema = schema::schema(&mut connection).await;
    let before_ddl: Vec<(String, String)> = sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE name IN ('source_artist_insert','source_artist_update') ORDER BY name")
        .fetch_all(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    let mut connection = connect(&path, 93).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        93
    );
    snapshot::assert_preserved(&mut connection, &before_rows).await;
    schema::source_artist_trigger_schema::assert_replacement(
        &mut connection,
        before_schema.clone(),
    )
    .await;
    for (name, old) in before_ddl {
        assert_ne!(
            old,
            schema::source_artist_trigger_schema::definition(&name),
            "each named trigger must actually change"
        );
    }
    integrity(&mut connection).await;
    through(93).run(&mut connection).await.unwrap();
    snapshot::assert_preserved(&mut connection, &before_rows).await;
    connection.close().await.unwrap();
    let mut connection = connect(&path, 93).await;
    snapshot::assert_preserved(&mut connection, &before_rows).await;
    schema::source_artist_trigger_schema::assert_replacement(&mut connection, before_schema).await;
    integrity(&mut connection).await;
    let mut fresh = schema::reference_93().await;
    assert_eq!(
        schema::schema(&mut connection).await,
        schema::schema(&mut fresh).await,
        "fresh and upgraded93 must have identical schema"
    );
    fresh.close().await.unwrap();
    connection.close().await.unwrap();
}

async fn integrity(connection: &mut sqlx::SqliteConnection) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pragma_foreign_key_check")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(connection)
            .await
            .unwrap(),
        "ok"
    );
}
