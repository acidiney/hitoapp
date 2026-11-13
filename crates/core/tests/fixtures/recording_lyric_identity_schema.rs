use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["recording_lyric_identities"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "recording_lyric_identities"),
    ("index", "sqlite_autoindex_recording_lyric_identities_1"),
    ("index", "recording_lyric_identity_exact"),
    ("index", "recording_lyric_identities_recording"),
    ("index", "recording_lyric_identities_retention"),
    ("index", "recording_lyric_identities_installation"),
    ("trigger", "recording_lyric_identity_owner"),
    ("trigger", "recording_lyric_identity_limit"),
    ("trigger", "recording_lyric_identity_immutable"),
];

const DECLARATIONS: &[&str] = &[
    "CREATE TABLE recording_lyric_identities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    provider_recording_id TEXT NOT NULL,
    provider_revision TEXT,
    reviewed_metadata TEXT NOT NULL CHECK(length(CAST(reviewed_metadata AS BLOB)) <= 32768 AND json_valid(reviewed_metadata)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision >= 1),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
)",
    "CREATE UNIQUE INDEX recording_lyric_identity_exact ON recording_lyric_identities(
    user_id,recording_id,installation_id,provider_recording_id,ifnull(provider_revision,''))",
    "CREATE INDEX recording_lyric_identities_recording ON recording_lyric_identities(user_id,recording_id,id)",
    "CREATE INDEX recording_lyric_identities_retention ON recording_lyric_identities(recording_id)",
    "CREATE INDEX recording_lyric_identities_installation ON recording_lyric_identities(installation_id)",
    "CREATE TRIGGER recording_lyric_identity_owner BEFORE INSERT ON recording_lyric_identities
WHEN NOT EXISTS(SELECT 1 FROM plugin_installations installation
    JOIN recordings recording ON recording.id=new.recording_id
    JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=new.user_id
    WHERE installation.id=new.installation_id AND installation.user_id=new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Lyric identities require an owned installation and recording access');
END",
    "CREATE TRIGGER recording_lyric_identity_limit BEFORE INSERT ON recording_lyric_identities
WHEN (SELECT count(*) FROM recording_lyric_identities WHERE user_id=new.user_id AND recording_id=new.recording_id)>=8 BEGIN
    SELECT RAISE(ABORT, 'A recording can retain at most eight independent lyric identities per account');
END",
    "CREATE TRIGGER recording_lyric_identity_immutable BEFORE UPDATE ON recording_lyric_identities BEGIN
    SELECT RAISE(ABORT, 'Reviewed lyric identities are immutable');
END",
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    for declaration in DECLARATIONS {
        sqlx::raw_sql(declaration)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0095 must preserve every prior schema object"
        );
    }
    let mut added = after
        .iter()
        .filter(|object| {
            !before
                .iter()
                .any(|old| old.0 == object.0 && old.1 == object.1)
        })
        .map(|object| (object.0.as_str(), object.1.as_str()))
        .collect::<Vec<_>>();
    let mut expected = ADDED.to_vec();
    added.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        added, expected,
        "0095 must add exactly the declared private lyric identity schema"
    );
    connection
}

pub async fn assert_empty(connection: &mut SqliteConnection) {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_lyric_identities")
        .fetch_one(connection)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "historical state must not invent private reviewed lyric identities"
    );
}
