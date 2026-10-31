#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn name_matches_made_before_the_coverage_rule_are_rule_two_and_later_ones_rule_three() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("name-match-rule-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(72).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0);
         PRAGMA foreign_keys=OFF;
         INSERT INTO artist_association_name_matches(association_id,evidence_kind,receipt_order,created_at) VALUES('before','name_discography',1,100);
         INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule) VALUES('one','r1','o1',3);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(73).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO artist_association_name_matches(association_id,evidence_kind,receipt_order,created_at) VALUES('after','name_discography',2,4102444800);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(74).run(&mut connection).await.unwrap();

    let rules: Vec<(String, i64)> = sqlx::query_as(
        "SELECT association_id,rule FROM artist_association_name_matches ORDER BY association_id",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        rules,
        vec![("after".into(), 3), ("before".into(), 2)],
        "A match from before migration 0073 was installed is rule 2; a later one is rule 3"
    );
    let rerun: i64 =
        sqlx::query_scalar("SELECT rerun FROM artist_matching_jobs WHERE user_id='one'")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(rerun, 1, "The account walks again under the new rule");
    assert!(
        sqlx::query("UPDATE artist_association_name_matches SET rule=4")
            .execute(&mut connection)
            .await
            .is_err(),
        "Name match provenance stays immutable"
    );
    connection.close().await.unwrap();
}
