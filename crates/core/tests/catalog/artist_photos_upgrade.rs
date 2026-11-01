#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

const ARTIST: &str = "0f800000-0000-4000-8000-0000000000a1";

#[tokio::test]
async fn migration_0080_keeps_every_lookup_and_admits_artist_photos() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("artist-photos-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(79).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1);
         INSERT INTO cover_art_lookups(user_id,kind,mbid,state,attempts,next_attempt_at)
            VALUES('one','release','0f800000-0000-4000-8000-0000000000b1','queued',2,7);
         INSERT INTO cover_art_lookups(user_id,kind,mbid,state)
            VALUES('one','release_group','0f800000-0000-4000-8000-0000000000b2','missing');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    assert!(
        sqlx::query(&format!(
            "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES('one','artist','{ARTIST}','queued')"
        ))
        .execute(&mut connection)
        .await
        .is_err(),
        "an artist lookup is refused before 0080"
    );
    schema::through(80).run(&mut connection).await.unwrap();
    schema::assert_version(&mut connection, 80).await;
    let kept: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT kind,state,attempts,next_attempt_at FROM cover_art_lookups ORDER BY kind",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        kept,
        vec![
            ("release".into(), "queued".into(), 2, 7),
            ("release_group".into(), "missing".into(), 0, 0),
        ]
    );
    sqlx::query(&format!(
        "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES('one','artist','{ARTIST}','queued')"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES('one','playlist','0f800000-0000-4000-8000-0000000000b3','queued')"
        )
        .execute(&mut connection)
        .await
        .is_err(),
        "other kinds stay refused"
    );
    connection.close().await.unwrap();
}
