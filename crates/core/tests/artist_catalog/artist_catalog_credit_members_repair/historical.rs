use super::fixture::RepairFixture;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::Path;

type MemberChoiceRow = (
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);
type WholeChoiceRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);
type PersonalMetadataRow = (
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
);

pub(super) async fn capture(fixture: &mut RepairFixture) {
    let database = database_path(fixture);
    let selected = fixture.selected.as_mut().unwrap();
    selected.private_snapshot = Some(
        private_state(
            &database,
            &selected.recording,
            &selected.guest_request_id.unwrap().to_string(),
        )
        .await,
    );
    let override_case = fixture.override_case.as_mut().unwrap();
    override_case.private_snapshot = Some(
        private_state(
            &database,
            &override_case.recording,
            override_case.request_id.as_ref().unwrap(),
        )
        .await,
    );
    let singleton = fixture.singleton.as_mut().unwrap();
    singleton.credit_snapshot = Some(credit_identity_state(&database, &singleton.recording).await);
    let registration = fixture.registration.as_mut().unwrap();
    registration.credit_snapshot =
        Some(credit_identity_state(&database, &registration.recording).await);
}

pub(super) async fn remove_historical_projections(fixture: &RepairFixture) {
    let selected = fixture.selected.as_ref().unwrap();
    let singleton = fixture.singleton.as_ref().unwrap();
    let registration = fixture.registration.as_ref().unwrap();
    let database = database_path(fixture);
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let removed = sqlx::query(
        "DELETE FROM account_recording_credit_members WHERE recording_id IN (?,?,?) AND role='main'",
    )
    .bind(&selected.recording)
    .bind(&singleton.recording)
    .bind(&registration.recording)
    .execute(&mut connection)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(
        removed, 3,
        "the owned fixture must remove only its historical main projection"
    );
    let unused_main_artist = sqlx::query(
        "DELETE FROM account_artists
             WHERE id=? AND name='owned registration anchor' COLLATE BINARY
                AND NOT EXISTS (SELECT 1 FROM account_recording_credits reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_credit_keys reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_recording_credit_members reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_source_credits reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_releases reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM recording_artist_associations reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM recording_artist_member_identity_choices reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)",
    )
    .bind(&registration.main)
    .execute(&mut connection)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(
        unused_main_artist, 1,
        "the historical main artist used for registration repair must have no external references"
    );
    let missing_registration_artist_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_artists WHERE id=? AND name='owned registration anchor' COLLATE BINARY",
    )
    .bind(&registration.main)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(missing_registration_artist_rows, 0);
    let singleton_artist_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_artists WHERE id=? AND name='Owned singleton main' COLLATE BINARY",
    )
    .bind(&singleton.member)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        singleton_artist_rows, 1,
        "the singleton main artist remains because the whole credit references it"
    );
    let retained_featured_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_recording_credit_members WHERE user_id=(SELECT user_id FROM account_recording_credits WHERE recording_id=?) AND recording_id=? AND artist_id=? AND role='featured'",
    )
    .bind(&registration.recording)
    .bind(&registration.recording)
    .bind(&registration.guest)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(retained_featured_rows, 1);
    connection.close().await.unwrap();
}

pub(super) async fn private_state(
    database: &Path,
    recording: &str,
    request_id: &str,
) -> serde_json::Value {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let members: Vec<MemberChoiceRow> = sqlx::query_as("SELECT artist_id,role,revision,association_id,group_id,artist_operation_id,observation_id,retired
            FROM recording_artist_member_identity_choices WHERE recording_id=? ORDER BY artist_id")
        .bind(recording).fetch_all(&mut connection).await.unwrap();
    let whole: Vec<WholeChoiceRow> = sqlx::query_as("SELECT revision,association_id,group_id,artist_id,artist_operation_id,observation_id,retired
            FROM recording_artist_identity_choices WHERE recording_id=? ORDER BY user_id")
        .bind(recording).fetch_all(&mut connection).await.unwrap();
    let receipt: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest,receipt FROM artist_identity_choice_receipts WHERE request_id=?",
    )
    .bind(request_id)
    .fetch_optional(&mut connection)
    .await
    .unwrap();
    let personal: Option<PersonalMetadataRow> = sqlx::query_as(
        "SELECT override_title,override_artist,title,artist,title_operation_id,artist_operation_id
             FROM personal_recording_metadata WHERE recording_id=? ORDER BY user_id LIMIT 1",
    )
    .bind(recording)
    .fetch_optional(&mut connection)
    .await
    .unwrap();
    let member_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_recording_credit_members WHERE recording_id=?",
    )
    .bind(recording)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    serde_json::json!({"member_choices":members,"whole_choices":whole,"receipt":receipt,"personal":personal,"member_count":member_count})
}

pub(super) async fn artist_membership_state(
    database: &Path,
    recording: &str,
    artist: &str,
    name: &str,
) -> (i64, i64) {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let artists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_artists WHERE id=? AND name=? COLLATE BINARY",
    )
    .bind(artist)
    .bind(name)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    let memberships: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_recording_credit_members WHERE recording_id=? AND artist_id=?",
    )
    .bind(recording)
    .bind(artist)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    (artists, memberships)
}

pub(super) async fn credit_identity_state(database: &Path, recording: &str) -> serde_json::Value {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let row: (String, String, String) = sqlx::query_as(
        "SELECT credit.artist_id,key.artist_id,key.credit_key
         FROM account_recording_credits credit
         JOIN recordings recording ON recording.id=credit.recording_id
         JOIN recording_credit_observations observation
            ON observation.recording_id=recording.id AND observation.observation_id=recording.metadata_observation_id
         JOIN account_credit_keys key
            ON key.user_id=credit.user_id AND key.library_id=recording.library_id
                AND key.credit_key=observation.credit_key
         WHERE credit.recording_id=?",
    )
    .bind(recording)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    serde_json::json!({
        "whole_artist_id": row.0,
        "key_artist_id": row.1,
        "credit_key": row.2,
    })
}

fn database_path(fixture: &RepairFixture) -> std::path::PathBuf {
    fixture.directory.as_ref().unwrap().path().join("hito.db")
}
