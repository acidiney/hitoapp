#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn name_checks_decided_under_the_old_rule_expire_and_their_accounts_walk_again() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("credit-matches-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(70).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('one','one','hash',1),('two','two','hash',0),('three','three','hash',0);
         INSERT INTO artist_name_checks(user_id,recording_id,observation_id) VALUES('one','r1','o1'),('one','r2','o2'),('two','r3','o3');
         INSERT INTO artist_matching_jobs(user_id,phase,cursor,rerun) VALUES('two','provider','name:x',0);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    schema::through(71).run(&mut connection).await.unwrap();

    let checks: i64 = sqlx::query_scalar("SELECT count(*) FROM artist_name_checks")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        checks, 0,
        "Every name check is examined again under the score rule"
    );
    let jobs: Vec<(String, String, i64)> =
        sqlx::query_as("SELECT user_id,phase,rerun FROM artist_matching_jobs ORDER BY user_id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        jobs,
        vec![
            ("one".into(), "tags".into(), 0),
            ("two".into(), "provider".into(), 1)
        ],
        "Accounts that had checks walk again; an account with a walk under way reruns it; others are untouched"
    );
    for statement in [
        "INSERT INTO artist_association_credit_matches(association_id,evidence_kind,source_tracks,receipt_order) VALUES('missing','isrc',1,1)",
        "INSERT INTO artist_association_credit_matches(association_id,evidence_kind,source_tracks,receipt_order) VALUES('missing','guess',1,1)",
        "INSERT INTO artist_association_credit_matches(association_id,evidence_kind,source_tracks,receipt_order) VALUES('missing','isrc',0,1)",
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
