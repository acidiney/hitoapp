use super::{connect, grow, seed, through};
use sqlx::{Connection, SqliteConnection};

#[tokio::test]
async fn insert_update_move_and_conflict_keep_exact_artist_identity_and_library_ownership() {
    for (initial, upgrade) in [(92, false), (92, true), (93, false)] {
        let directory = tempfile::tempdir().unwrap();
        let mut connection = connect(&directory.path().join("owned.db"), initial).await;
        seed(&mut connection).await;
        grow(&mut connection, 1, 2).await;
        sqlx::raw_sql("INSERT INTO users(id,username,password_hash) VALUES ('other','other','owned-unused');
            INSERT INTO libraries(id,name,owner_id) VALUES ('other-library','Other library','other');
            INSERT INTO library_members(library_id,user_id) VALUES ('other-library','other');
            INSERT INTO recordings(id,library_id,title,artist) VALUES ('other-recording','other-library','Other track','Owned recording artist');
            INSERT INTO sources(id,recording_id,provider,provider_id) VALUES ('other-source','other-recording','local','owned-other');")
            .execute(&mut connection).await.unwrap();
        if upgrade {
            through(93).run(&mut connection).await.unwrap();
        }
        let original = artist(&mut connection, "library", "Owned recording artist").await;
        execute(&mut connection, "INSERT INTO source_metadata(source_id,title,artist,duration_ms) VALUES ('source-1','Owned track','Owned recording artist',1000)").await;
        assert_eq!(
            artist(&mut connection, "library", "Owned recording artist").await,
            original
        );
        execute(
            &mut connection,
            "UPDATE source_metadata SET artist='Exact' WHERE source_id='source-1'",
        )
        .await;
        let exact = artist(&mut connection, "library", "Exact").await;
        assert_eq!(exact.len(), 32);
        assert!(
            exact
                .bytes()
                .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
        );
        execute(&mut connection, "INSERT INTO source_metadata(source_id,title,artist,duration_ms) VALUES ('source-2','Owned duplicate','Exact',1000)").await;
        assert_eq!(artist(&mut connection, "library", "Exact").await, exact);
        execute(
            &mut connection,
            "UPDATE source_metadata SET artist='exact' WHERE source_id='source-1'",
        )
        .await;
        assert_ne!(artist(&mut connection, "library", "exact").await, exact);
        execute(
            &mut connection,
            "UPDATE source_metadata SET source_id='other-source' WHERE source_id='source-1'",
        )
        .await;
        assert_ne!(
            artist(&mut connection, "other-library", "exact").await,
            artist(&mut connection, "library", "exact").await
        );
        execute(&mut connection, "UPDATE source_metadata SET source_id='source-1',artist='Moved' WHERE source_id='other-source'").await;
        let wrong_owner: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM artists WHERE library_id='other-library' AND name='Moved'",
        )
        .fetch_one(&mut connection)
        .await
        .unwrap();
        assert_eq!(wrong_owner, 0);
        assert!(!artist(&mut connection, "library", "Moved").await.is_empty());
        assert!(sqlx::query("UPDATE source_metadata SET source_id='missing',artist='Invalid' WHERE source_id='source-1'").execute(&mut connection).await.is_err());
        assert!(sqlx::query("INSERT INTO source_metadata(source_id,title,artist,duration_ms) VALUES ('source-2','Owned duplicate','Invalid',1000)").execute(&mut connection).await.is_err());
        execute(&mut connection, "INSERT INTO source_metadata(source_id,title,artist,duration_ms) VALUES (NULL,'Owned unbound metadata','Unbound',1000)").await;
        let invalid: i64 =
            sqlx::query_scalar("SELECT count(*) FROM artists WHERE name IN ('Invalid','Unbound')")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        assert_eq!(invalid, 0);
        execute(
            &mut connection,
            "UPDATE source_metadata SET artist='Owned recording artist' WHERE source_id='source-1'",
        )
        .await;
        assert_eq!(
            artist(&mut connection, "library", "Owned recording artist").await,
            original
        );
        execute(&mut connection, "UPDATE source_metadata SET title='Changed title',duration_ms=2000 WHERE source_id='source-1'").await;
        execute(&mut connection, "DELETE FROM source_metadata").await;
        let retained: Vec<(String, String)> =
            sqlx::query_as("SELECT library_id,name FROM artists ORDER BY library_id,name")
                .fetch_all(&mut connection)
                .await
                .unwrap();
        assert_eq!(
            retained,
            vec![
                ("library".into(), "Exact".into()),
                ("library".into(), "Moved".into()),
                ("library".into(), "Owned recording artist".into()),
                ("library".into(), "exact".into()),
                ("other-library".into(), "Owned recording artist".into()),
                ("other-library".into(), "exact".into()),
            ]
        );
        let violations: i64 = sqlx::query_scalar("SELECT count(*) FROM pragma_foreign_key_check")
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(violations, 0);
        connection.close().await.unwrap();
    }
}

async fn execute(connection: &mut SqliteConnection, statement: &str) {
    sqlx::query(statement).execute(connection).await.unwrap();
}

async fn artist(connection: &mut SqliteConnection, library: &str, name: &str) -> String {
    sqlx::query_scalar("SELECT id FROM artists WHERE library_id=? AND name=?")
        .bind(library)
        .bind(name)
        .fetch_one(connection)
        .await
        .unwrap()
}
