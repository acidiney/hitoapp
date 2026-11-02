#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

const ARTIST: &str = "0f800000-0000-4000-8000-0000000000a1";
const CREDIT: &str = "0123456789abcdef0123456789abcdef";

fn insert(kind: &str, identifier: &str) -> String {
    format!(
        "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES('one','{kind}','{identifier}','queued')"
    )
}

#[tokio::test]
async fn migration_0081_keeps_every_lookup_and_admits_credited_artist_photos() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("credited-photos-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(80).run(&mut connection).await.unwrap();
    sqlx::raw_sql(&format!(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);
         INSERT INTO cover_art_lookups(user_id,kind,mbid,state,attempts,next_attempt_at)
            VALUES('one','artist','{ARTIST}','queued',2,7);
         INSERT INTO cover_art_lookups(user_id,kind,mbid,state)
            VALUES('one','release_group','0f800000-0000-4000-8000-0000000000b2','missing');"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    assert!(
        sqlx::query(&insert("credited_artist", CREDIT))
            .execute(&mut connection)
            .await
            .is_err(),
        "a credited artist lookup is refused before 0081"
    );
    schema::through(81).run(&mut connection).await.unwrap();
    schema::assert_version(&mut connection, 81).await;
    let kept: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT kind,state,attempts,next_attempt_at FROM cover_art_lookups ORDER BY kind",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        kept,
        vec![
            ("artist".into(), "queued".into(), 2, 7),
            ("release_group".into(), "missing".into(), 0, 0),
        ]
    );
    sqlx::query(&insert("credited_artist", CREDIT))
        .execute(&mut connection)
        .await
        .unwrap();
    for refused in [
        insert("credited_artist", ""),
        insert("credited_artist", &"a".repeat(65)),
        insert("artist", CREDIT),
        insert("release", CREDIT),
        insert("playlist", ARTIST),
    ] {
        assert!(
            sqlx::query(&refused)
                .execute(&mut connection)
                .await
                .is_err(),
            "{refused}"
        );
    }
    connection.close().await.unwrap();
}

#[tokio::test]
async fn migration_0081_keeps_ready_running_and_unavailable_lookups_whole() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("credited-photos-rows.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(80).run(&mut connection).await.unwrap();
    let hash = "ab".repeat(32);
    sqlx::raw_sql(&format!(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);
         INSERT INTO artwork_assets(user_id,content_hash,content_type,width,height,bytes)
            VALUES('one','{hash}','image/png',1,1,x'89504e47');
         INSERT INTO cover_art_lookups(user_id,kind,mbid,state,content_hash,attempts,next_attempt_at,lease_until,updated_at)
            VALUES('one','artist','0f800000-0000-4000-8000-0000000000a1','ready','{hash}',0,0,NULL,1000);
         INSERT INTO cover_art_lookups(user_id,kind,mbid,state,attempts,next_attempt_at,lease_until,updated_at)
            VALUES('one','release','0f800000-0000-4000-8000-0000000000a2','running',2,40,5000,2000);
         INSERT INTO cover_art_lookups(user_id,kind,mbid,state,attempts,next_attempt_at,lease_until,updated_at)
            VALUES('one','release_group','0f800000-0000-4000-8000-0000000000a3','unavailable',4,90,NULL,3000);"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    type Row = (String, String, Option<String>, i64, i64, Option<i64>, i64);
    let read = "SELECT kind,state,content_hash,attempts,next_attempt_at,lease_until,updated_at FROM cover_art_lookups ORDER BY kind";
    let before: Vec<Row> = sqlx::query_as(read)
        .fetch_all(&mut connection)
        .await
        .unwrap();

    schema::through(81).run(&mut connection).await.unwrap();

    let after: Vec<Row> = sqlx::query_as(read)
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(after.len(), 3);
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert!(violations.is_empty());
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'cover_art_lookups' AND sql LIKE '%WHERE%' ORDER BY name",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        indexes,
        ["cover_art_lookups_asset", "cover_art_lookups_queue"]
    );
    connection.close().await.unwrap();
}
