#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

const SEED: &str = "
INSERT INTO users(id,username,password_hash,admin) VALUES('legacy','legacy','hash',1),('second','second','hash',1);
INSERT INTO libraries(id,name,owner_id) VALUES('library','Library','legacy');
INSERT INTO recordings(id,library_id,title,artist,duration_ms) VALUES('recording','library','Title','Artist',1000);
INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms)
    VALUES('observation','recording','local','legacy_normalized','Title','Artist',1000);
INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,provider_artist_id) VALUES('group','legacy','builtin_musicbrainz','artist');
INSERT INTO artist_identity_choice_receipts(user_id,request_id,request_digest,receipt) VALUES('legacy','request','digest','{}');
INSERT INTO artist_identity_changes(id,user_id,group_id,revision,kind,receipt_order) VALUES('change','legacy','group',1,'split',1);
INSERT INTO shared_artist_corrections(seq,id,promoted_by,source_kind,source_change_id,change_floor) VALUES
    (1,'first','legacy','split','change',0),(2,'second','second','split','change',3),(5,'fifth','legacy','split','change',4);
INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist)
    VALUES(1,'recording','observation','artist','Artist'),(5,'recording','observation','artist','Artist');
INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES
    (1,1,1,'legacy'),(1,2,0,'second'),(2,1,1,'second'),(5,1,1,'legacy');
";

const READ: [&str; 3] = [
    "SELECT json_group_array(json_array(seq,id,promoted_by,source_kind,source_change_id,source_association_id,change_floor,created_at)) FROM (SELECT * FROM shared_artist_corrections ORDER BY seq)",
    "SELECT json_group_array(json_array(correction_seq,revision,applied,changed_by,created_at)) FROM (SELECT * FROM shared_artist_correction_states ORDER BY correction_seq,revision)",
    "SELECT json_group_array(json_array(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist,retired_reason)) FROM (SELECT * FROM shared_artist_correction_subjects ORDER BY correction_seq)",
];

async fn rows(connection: &mut SqliteConnection) -> Vec<String> {
    let mut all = Vec::new();
    for statement in READ {
        all.push(
            sqlx::query_scalar(statement)
                .fetch_one(&mut *connection)
                .await
                .unwrap(),
        );
    }
    all
}

async fn rejected(connection: &mut SqliteConnection, statement: &str) {
    assert!(
        sqlx::query(statement).execute(connection).await.is_err(),
        "{statement} must be rejected"
    );
}

#[tokio::test]
async fn shared_corrections_survive_the_rebuild_and_gain_only_narrow_erasure_paths() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("correction-erasure-upgrade.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(64).run(&mut connection).await.unwrap();
    sqlx::raw_sql(SEED).execute(&mut connection).await.unwrap();
    let before = rows(&mut connection).await;
    let sequence: i64 = sqlx::query_scalar(
        "SELECT seq FROM sqlite_sequence WHERE name='shared_artist_corrections'",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(sequence, 5);
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options.clone().create_if_missing(false))
        .await
        .unwrap();
    schema::through(65).run(&mut connection).await.unwrap();
    let mut reference = schema::reference_65().await;
    assert!(
        schema::schema(&mut connection).await == schema::schema(&mut reference).await,
        "The migrated schema matches the exact declared 0065 schema"
    );
    reference.close().await.unwrap();
    assert_eq!(
        rows(&mut connection).await,
        before,
        "Every shared correction, subject and state keeps its exact values"
    );
    let violations: Vec<(String,)> =
        sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert!(violations.is_empty());
    let sequence: i64 = sqlx::query_scalar(
        "SELECT seq FROM sqlite_sequence WHERE name='shared_artist_corrections'",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        sequence, 5,
        "The correction numbering high-water mark is kept"
    );
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM correction_erasure_scopes")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(scopes, 0);

    let next: i64 = sqlx::query_scalar(
        "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,change_floor) VALUES('sixth','legacy','split','change',5) RETURNING seq",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(next, 6);
    for statement in [
        "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,change_floor) VALUES('x','legacy','split',0)",
        "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,change_floor) VALUES('x',NULL,'split','change',0)",
        "INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,source_association_id,change_floor) VALUES('x','legacy','split','change','association',0)",
        "INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES(6,1,1,NULL)",
        "UPDATE shared_artist_corrections SET promoted_by=NULL",
        "UPDATE shared_artist_corrections SET change_floor=9",
        "UPDATE shared_artist_correction_states SET changed_by=NULL",
        "UPDATE shared_artist_correction_states SET applied=0",
        "DELETE FROM shared_artist_corrections",
        "DELETE FROM shared_artist_correction_states",
        "DELETE FROM shared_artist_correction_subjects",
        "DELETE FROM artist_identity_changes",
        "DELETE FROM artist_identity_choice_receipts",
    ] {
        rejected(&mut connection, statement).await;
    }

    sqlx::query("INSERT INTO correction_erasure_scopes(kind,subject) VALUES('account','legacy')")
        .execute(&mut connection)
        .await
        .unwrap();
    for statement in [
        "UPDATE shared_artist_corrections SET change_floor=9 WHERE promoted_by='legacy'",
        "UPDATE shared_artist_correction_states SET applied=0 WHERE changed_by='legacy'",
        "UPDATE shared_artist_corrections SET promoted_by=NULL WHERE promoted_by='second'",
        "DELETE FROM shared_artist_correction_subjects",
    ] {
        rejected(&mut connection, statement).await;
    }
    sqlx::query(
        "UPDATE shared_artist_correction_states SET changed_by=NULL WHERE changed_by='legacy'",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::query("UPDATE shared_artist_corrections SET promoted_by=NULL,source_change_id=NULL WHERE promoted_by='legacy'")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("DELETE FROM correction_erasure_scopes")
        .execute(&mut connection)
        .await
        .unwrap();
    let anonymous: (i64, i64) =
        sqlx::query_as("SELECT count(*), sum(promoted_by IS NULL) FROM shared_artist_corrections")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(anonymous, (4, 3));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn an_empty_library_upgrades_to_the_exact_erasure_schema() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("correction-erasure-empty.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    schema::through(64).run(&mut connection).await.unwrap();
    schema::through(65).run(&mut connection).await.unwrap();
    schema::assert_version(&mut connection, 65).await;
    connection.close().await.unwrap();
}
