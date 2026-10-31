#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn name_checks_stay_recorded_and_their_accounts_walk_again_under_the_coverage_rule() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("coverage-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(72).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0),('three','three','hash',0);
         INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule) VALUES('one','r1','o1',2),('one','r2','o2',1),('two','r3','o3',2);
         INSERT INTO artist_matching_jobs(user_id,phase,cursor,rerun,attempts) VALUES('two','provider','name:x',0,3);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(73).run(&mut connection).await.unwrap();

    let checks: Vec<(String, i64)> =
        sqlx::query_as("SELECT recording_id,rule FROM artist_name_checks ORDER BY recording_id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        checks,
        vec![("r1".into(), 2), ("r2".into(), 1), ("r3".into(), 2)],
        "Checks keep their rows and their rules; the current rule ignores them"
    );
    let jobs: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT user_id,phase,rerun,attempts FROM artist_matching_jobs ORDER BY user_id",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        jobs,
        vec![
            ("one".into(), "tags".into(), 0, 0),
            ("two".into(), "provider".into(), 1, 3)
        ],
        "Accounts that had checks walk again; an account with a walk under way reruns it; others are untouched"
    );
    for statement in [
        "INSERT INTO artist_association_coverage_matches(association_id,matched_tracks,probed_tracks,receipt_order) VALUES('missing',0,0,1)",
        "INSERT INTO artist_association_coverage_matches(association_id,matched_tracks,probed_tracks,receipt_order) VALUES('missing',5,4,1)",
        "INSERT INTO artist_association_coverage_matches(association_id,matched_tracks,probed_tracks,receipt_order) VALUES('missing',5,6,1)",
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
