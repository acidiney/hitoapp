#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn name_checks_of_the_earlier_rule_stay_recorded_as_such_and_their_accounts_walk_again() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("name-rule-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(71).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0),('three','three','hash',0);
         INSERT INTO artist_name_checks(user_id,recording_id,observation_id) VALUES('one','r1','o1'),('one','r2','o2'),('two','r3','o3');
         INSERT INTO artist_matching_jobs(user_id,phase,cursor,rerun,attempts) VALUES('two','provider','name:x',0,3);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(72).run(&mut connection).await.unwrap();

    let checks: Vec<(String, i64)> =
        sqlx::query_as("SELECT recording_id,rule FROM artist_name_checks ORDER BY recording_id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        checks,
        vec![("r1".into(), 1), ("r2".into(), 1), ("r3".into(), 1)],
        "Checks keep their rows and are recorded under rule 1, which the current rule ignores"
    );
    let jobs: Vec<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT user_id,phase,rerun,unavailable,attempts FROM artist_matching_jobs ORDER BY user_id",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        jobs,
        vec![
            ("one".into(), "tags".into(), 0, 0, 0),
            ("two".into(), "provider".into(), 1, 0, 3)
        ],
        "Accounts that had checks walk again; an account with a walk under way reruns it; others are untouched"
    );
    assert!(
        sqlx::query("UPDATE artist_matching_jobs SET unavailable=2 WHERE user_id='one'")
            .execute(&mut connection)
            .await
            .is_err(),
        "The unavailable flag is a boolean"
    );
    connection.close().await.unwrap();
}
