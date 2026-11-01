#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn only_follows_authorized_at_upgrade_receive_an_approval_baseline() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("follow-approval-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(78).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);
         INSERT INTO plugin_services(id,name,base_url,revision) VALUES('svc','S','http://s.invalid/',1);
         INSERT INTO plugin_installations(id,user_id,service_id,manifest,revision)
            VALUES('inst','one','svc','{\"id\":\"p\",\"name\":\"P\",\"protocol_version\":1,\"capabilities\":[\"playlists\",\"search\"]}',3);
         INSERT INTO playlists(id,user_id,name,kind) VALUES('current','one','C','followed'),('paused','one','P','followed');
         INSERT INTO playlist_follows(playlist_id,user_id,installation_id,installation_revision,service_id,service_revision,
            snapshot_installation_revision,snapshot_service_revision,provider,provider_playlist_id,snapshot,content_digest,attribution,interval_seconds,next_refresh_at)
            VALUES('current','one','inst',3,'svc',1,3,1,'p','x','s',hex(zeroblob(32)),'{}',3600,0),
                  ('paused','one','inst',2,'svc',1,2,1,'p','y','s',hex(zeroblob(32)),'{}',3600,0);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(79).run(&mut connection).await.unwrap();
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT playlist_id,approved_capabilities FROM playlist_follows ORDER BY playlist_id",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(rows[0].0, "current");
    assert!(rows[0].1.as_deref().unwrap().contains("search"));
    assert_eq!(rows[1], ("paused".into(), None));
    connection.close().await.unwrap();
}
