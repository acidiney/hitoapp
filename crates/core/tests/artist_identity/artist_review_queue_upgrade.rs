#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn migration_0075_adds_the_review_tables_and_walks_accounts_with_name_checks_again() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("review-queue-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(74).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0),('three','three','hash',0);
         INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule) VALUES('one','r1','o1',4),('two','r2','o2',4);
         INSERT INTO artist_matching_jobs(user_id,phase,cursor,rerun,attempts) VALUES('two','provider','name:x',0,3);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(75).run(&mut connection).await.unwrap();

    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN
         ('artist_name_candidates','artist_review_dismissals','artist_association_candidate_confirmations')",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(tables, 3, "The review tables exist");
    let checks: Vec<(String, i64)> =
        sqlx::query_as("SELECT recording_id,rule FROM artist_name_checks ORDER BY recording_id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        checks,
        vec![("r1".into(), 4), ("r2".into(), 4)],
        "Checks keep their rows and rules"
    );
    let jobs: Vec<(String, i64, i64)> =
        sqlx::query_as("SELECT user_id,rerun,attempts FROM artist_matching_jobs ORDER BY user_id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        jobs,
        vec![("one".into(), 0, 0), ("two".into(), 1, 3)],
        "Accounts with checks walk again; a walk under way is rerun; others are untouched"
    );
    for count in [
        "SELECT count(*) FROM artist_name_candidates",
        "SELECT count(*) FROM artist_review_dismissals",
        "SELECT count(*) FROM artist_association_candidate_confirmations",
    ] {
        let rows: i64 = sqlx::query_scalar(count)
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(rows, 0, "{count} implies nothing");
    }
    for statement in [
        "INSERT INTO artist_association_candidate_confirmations(association_id,evidence_kind,receipt_order) VALUES('missing','name_discography',1)",
        "INSERT INTO artist_name_candidates(user_id,artist_id,provider_artist_id,position,name,corroborated_titles) VALUES('nobody','credit','mb',0,'Name',0)",
        "INSERT INTO artist_review_dismissals(user_id,artist_id,provider_artist_id) VALUES('nobody','credit','mb')",
    ] {
        assert!(
            sqlx::query(statement)
                .execute(&mut connection)
                .await
                .is_err(),
            "{statement} must be rejected"
        );
    }
    sqlx::raw_sql(
        "PRAGMA foreign_keys=OFF;
         INSERT INTO artist_association_candidate_confirmations(association_id,evidence_kind,receipt_order) VALUES('a1','user_confirmed_candidate',1);
         PRAGMA foreign_keys=ON;",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    for statement in [
        "UPDATE artist_association_candidate_confirmations SET receipt_order=2",
        "DELETE FROM artist_association_candidate_confirmations",
    ] {
        assert!(
            sqlx::query(statement)
                .execute(&mut connection)
                .await
                .is_err(),
            "{statement} must be rejected"
        );
    }
    connection.close().await.unwrap();
}
