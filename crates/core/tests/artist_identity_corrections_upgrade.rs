#[path = "fixtures/collection_artwork_schema.rs"]
mod schema;

use schema::correction_schema;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

async fn rejected(connection: &mut SqliteConnection, statements: &[&str]) {
    for statement in statements {
        assert!(
            sqlx::query(statement)
                .execute(&mut *connection)
                .await
                .is_err(),
            "{statement} must be rejected"
        );
    }
}

async fn accepted(connection: &mut SqliteConnection, statements: &[&str]) {
    for statement in statements {
        sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .unwrap_or_else(|error| panic!("{statement} must be accepted: {error}"));
    }
}

#[tokio::test]
async fn artist_corrections_are_added_without_touching_existing_accounts_or_reviews() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("artist-corrections-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(58).run(&mut connection).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES('legacy-admin','legacy','hash',1);
         INSERT INTO users(id,username,password_hash,admin) VALUES('plain-user','plain','hash',0);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let before = schema::schema(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options.clone().create_if_missing(false))
        .await
        .unwrap();
    schema::through(59).run(&mut connection).await.unwrap();
    let after = schema::schema(&mut connection).await;
    assert!(before.iter().all(|object| after.contains(object)));
    let users: Vec<(String, bool)> = sqlx::query_as("SELECT id,admin FROM users ORDER BY id")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        users,
        vec![("legacy-admin".into(), true), ("plain-user".into(), false)]
    );
    schema::assert_current(&mut connection).await;
    correction_schema::assert_empty(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("PRAGMA foreign_keys=OFF")
        .execute(&mut connection)
        .await
        .unwrap();
    rejected(
        &mut connection,
        &[
            "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_association_id,change_floor) VALUES('c0','plain-user','unlink','a1',0)",
            "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,change_floor) VALUES('c0','legacy-admin','unlink','x',0)",
            "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_association_id,change_floor) VALUES('c0','legacy-admin','split','a1',0)",
            "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_association_id,change_floor) VALUES('c0','legacy-admin','unlink','a1',-1)",
        ],
    )
    .await;
    accepted(
        &mut connection,
        &[
            "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_association_id,change_floor) VALUES('c1','legacy-admin','unlink','a1',0)",
            "INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES(1,1,1,'legacy-admin')",
            "INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist) VALUES(1,'r1','o1','artist','Raw')",
            "INSERT INTO shared_artist_correction_receipts(user_id,request_id,request_digest,receipt) VALUES('legacy-admin','q1','d','{}')",
            "INSERT INTO artist_association_change_retirements(user_id,association_id,reason) VALUES('plain-user','a1','credit_changed')",
        ],
    )
    .await;
    rejected(
        &mut connection,
        &[
            "INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES(1,2,0,'plain-user')",
            "INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES(1,0,0,'legacy-admin')",
            "INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES(1,2,2,'legacy-admin')",
            "UPDATE shared_artist_correction_states SET applied=0",
            "DELETE FROM shared_artist_correction_states",
            "UPDATE shared_artist_corrections SET change_floor=9",
            "DELETE FROM shared_artist_corrections",
            "UPDATE shared_artist_correction_subjects SET raw_artist='Other'",
            "UPDATE shared_artist_correction_subjects SET retired_reason='unknown'",
            "DELETE FROM shared_artist_correction_subjects",
            "UPDATE shared_artist_correction_receipts SET receipt='{}'",
            "DELETE FROM shared_artist_correction_receipts",
            "UPDATE artist_association_change_retirements SET reason='credit_changed'",
            "DELETE FROM artist_association_change_retirements",
            "INSERT INTO artist_association_change_retirements(user_id,association_id,reason) VALUES('plain-user','a2','other')",
        ],
    )
    .await;
    accepted(
        &mut connection,
        &["UPDATE shared_artist_correction_subjects SET retired_reason='tags_changed'"],
    )
    .await;
    rejected(
        &mut connection,
        &["UPDATE shared_artist_correction_subjects SET retired_reason='tags_changed'"],
    )
    .await;

    accepted(
        &mut connection,
        &[
            "INSERT INTO artist_identity_changes(id,user_id,group_id,revision,kind,receipt_order) VALUES('h1','plain-user','g1',1,'split',1)",
            "INSERT INTO artist_identity_change_items(change_id,recording_id,from_revision,from_artist_id,to_revision) VALUES('h1','r1',1,'artist',2)",
        ],
    )
    .await;
    rejected(
        &mut connection,
        &[
            "INSERT INTO artist_identity_changes(id,user_id,group_id,revision,kind,receipt_order) VALUES('h2','plain-user','g1',2,'merge',1)",
            "INSERT INTO artist_identity_changes(id,user_id,group_id,revision,kind,receipt_order) VALUES('h3','plain-user','g1',1,'undo',1)",
            "UPDATE artist_identity_changes SET kind='undo'",
            "DELETE FROM artist_identity_changes",
            "INSERT INTO artist_identity_change_items(change_id,recording_id,from_revision,from_artist_id,to_revision) VALUES('h1','r2',2,'artist',2)",
            "UPDATE artist_identity_change_items SET to_revision=9",
            "UPDATE artist_identity_change_items SET retired_reason='other'",
            "DELETE FROM artist_identity_change_items",
        ],
    )
    .await;
    accepted(
        &mut connection,
        &["UPDATE artist_identity_change_items SET retired_reason='credit_changed'"],
    )
    .await;
    rejected(
        &mut connection,
        &["UPDATE artist_identity_change_items SET retired_reason='credit_changed'"],
    )
    .await;
}
