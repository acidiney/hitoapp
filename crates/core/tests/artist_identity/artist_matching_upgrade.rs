#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn artist_matching_is_added_without_implying_matches_or_dismissals() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("artist-matching-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(59).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('legacy','legacy','hash',1);
",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options.clone().create_if_missing(false))
        .await
        .unwrap();
    schema::through(60).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    assert!(before.iter().all(|object| after.contains(object)));
    let mut reference = schema::reference_60().await;
    assert!(
        schema::schema(&mut connection).await == schema::schema(&mut reference).await,
        "the migrated schema must match the declared 0060 schema exactly"
    );
    schema::assert_version(&mut connection, 60).await;
    sqlx::raw_sql(
        "INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,plugin_installation_id,provider_artist_id)
         VALUES('g1','legacy','builtin_musicbrainz',NULL,'11111111-1111-4111-8111-111111111111');",
    )
    .execute(&mut connection)
    .await
    .unwrap();

    for statement in [
        "INSERT INTO artist_association_matches(association_id,evidence_kind,receipt_order) VALUES('missing','file_tag_artist_id',1)",
        "INSERT INTO artist_match_dismissals(user_id,artist_id,group_id) VALUES('legacy','credit','missing')",
        "INSERT INTO artist_match_dismissals(user_id,artist_id,group_id) VALUES('nobody','credit','g1')",
    ] {
        assert!(
            sqlx::query(statement)
                .execute(&mut connection)
                .await
                .is_err(),
            "{statement} must be rejected"
        );
    }
    sqlx::query("INSERT INTO artist_match_dismissals(user_id,artist_id,group_id) VALUES('legacy','credit','g1')")
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(
        sqlx::query("INSERT INTO artist_match_dismissals(user_id,artist_id,group_id) VALUES('legacy','credit','g1')")
            .execute(&mut connection)
            .await
            .is_err(),
        "A dismissal is remembered once per account, credit and chosen artist"
    );
    sqlx::query("DELETE FROM artist_match_dismissals")
        .execute(&mut connection)
        .await
        .expect("A dismissal can be restored");
    connection.close().await.unwrap();
}
