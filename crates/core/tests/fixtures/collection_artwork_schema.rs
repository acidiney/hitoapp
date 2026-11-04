#![allow(dead_code)]

use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

#[path = "account_erasure_schema.rs"]
pub mod account_schema;
#[path = "album_edition_schema.rs"]
pub mod album_schema;
#[path = "artist_images_schema.rs"]
pub mod artist_images_schema;
#[path = "automatic_metadata_schema.rs"]
pub mod automation_schema;
#[path = "artist_association_changes_schema.rs"]
pub mod changes_schema;
#[path = "artist_identity_correction_schema.rs"]
pub mod correction_schema;
#[path = "coverage_matches_schema.rs"]
pub mod coverage_matches_schema;
#[path = "credit_matches_schema.rs"]
pub mod credit_matches_schema;
#[path = "credited_artist_photos_schema.rs"]
pub mod credited_artist_photos_schema;
#[path = "credited_artists_schema.rs"]
pub mod credited_artists_schema;
#[path = "correction_erasure_schema.rs"]
pub mod erasure_schema;
#[path = "follow_approval_schema.rs"]
pub mod follow_approval_schema;
#[path = "source_genres_schema.rs"]
pub mod genre_schema;
#[path = "artist_identity_selection_schema.rs"]
pub mod identity_schema;
#[path = "artist_match_schema.rs"]
pub mod match_schema;
#[path = "musicbrainz_genres_schema.rs"]
pub mod musicbrainz_genres_schema;
#[path = "name_match_rule_schema.rs"]
pub mod name_match_rule_schema;
#[path = "artist_name_match_schema.rs"]
pub mod name_match_schema;
#[path = "name_rule_schema.rs"]
pub mod name_rule_schema;
#[path = "pin_lock_schema.rs"]
pub mod pin_schema;
#[path = "plugin_authorization_status_schema.rs"]
pub mod plugin_authorization_status_schema;
#[path = "artist_provider_evidence_schema.rs"]
pub mod provider_evidence_schema;
#[path = "artist_provider_url_schema.rs"]
pub mod provider_url_schema;
#[path = "recording_groups_schema.rs"]
pub mod recording_groups_schema;
#[path = "recording_id_matches_schema.rs"]
pub mod recording_id_matches_schema;
#[path = "review_queue_schema.rs"]
pub mod review_queue_schema;
#[path = "session_client_schema.rs"]
pub mod session_client_schema;
#[path = "source_unavailability_schema.rs"]
pub mod source_unavailability_schema;
#[path = "musicbrainz_tag_identity_schema.rs"]
pub mod tag_identity_schema;

pub type SchemaObject = (String, String, String, Option<String>);
type Column = (i64, String, String, i64, Option<String>, i64, i64);
type ForeignKey = (i64, i64, String, String, String, String, String, String);

pub const REPLACED: &[(&str, &str)] = &[
    ("table", "artwork_fetches"),
    ("table", "recording_artwork"),
    ("index", "artwork_pending_recording"),
    ("trigger", "artwork_fetch_identity"),
    ("trigger", "recording_artwork_owner"),
    ("trigger", "recording_artwork_update"),
    ("view", "current_artwork_fetch_authority"),
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "account_community_preferences"),
    ("index", "sqlite_autoindex_account_community_preferences_1"),
    ("table", "release_artwork"),
    ("index", "sqlite_autoindex_release_artwork_1"),
    ("index", "sqlite_autoindex_release_artwork_2"),
    ("table", "playlist_artwork"),
    ("index", "sqlite_autoindex_playlist_artwork_1"),
    ("index", "sqlite_autoindex_playlist_artwork_2"),
    ("index", "artwork_pending_release"),
    ("index", "artwork_pending_playlist"),
    ("index", "artwork_history"),
    ("index", "artwork_history_account"),
    ("index", "release_artwork_asset"),
    ("index", "playlist_artwork_asset"),
    ("view", "current_artwork_fetch_connections"),
    ("view", "retained_artwork_references"),
    ("trigger", "artwork_fetch_target_insert"),
    ("trigger", "artwork_fetch_retention_insert"),
    ("trigger", "artwork_fetch_retention_update"),
    ("trigger", "recording_artwork_retain"),
    ("trigger", "recording_artwork_replace"),
    ("trigger", "recording_artwork_unretain"),
    ("trigger", "release_artwork_owner"),
    ("trigger", "release_artwork_update"),
    ("trigger", "release_artwork_retain"),
    ("trigger", "release_artwork_replace"),
    ("trigger", "release_artwork_unretain"),
    ("trigger", "playlist_artwork_owner"),
    ("trigger", "playlist_artwork_update"),
    ("trigger", "playlist_artwork_retain"),
    ("trigger", "playlist_artwork_replace"),
    ("trigger", "playlist_artwork_unretain"),
    ("table", "community_reporters"),
    ("index", "sqlite_autoindex_community_reporters_1"),
    ("index", "community_credential_fingerprints"),
    ("table", "community_reports"),
    ("index", "sqlite_autoindex_community_reports_1"),
    ("index", "community_report_queue"),
    ("index", "community_report_account"),
    ("index", "community_report_session"),
    ("trigger", "community_report_identity"),
    ("trigger", "community_reporting_disabled"),
    ("trigger", "community_reporting_logout"),
    ("trigger", "community_reporting_session_changed"),
];

pub const RANKING_ADDED: &[(&str, &str)] = &[
    ("table", "account_community_ranking_preferences"),
    (
        "index",
        "sqlite_autoindex_account_community_ranking_preferences_1",
    ),
];
pub const RANKING_TABLE: &str = "account_community_ranking_preferences";

pub const LEGACY_JOB_COLUMNS: &[&str] = &[
    "id",
    "user_id",
    "recording_id",
    "session_hash",
    "observation_id",
    "binding_id",
    "binding_revision",
    "installation_id",
    "installation_revision",
    "service_id",
    "service_revision",
    "status",
    "attempts",
    "reservation_bytes",
    "next_attempt_at",
    "lease_token",
    "lease_until",
    "error",
    "created_at",
    "updated_at",
];

pub const NEW_TABLES: &[&str] = &[
    "account_community_preferences",
    "release_artwork",
    "playlist_artwork",
    "community_reporters",
    "community_reports",
];
pub const NEW_JOB_COLUMNS: &[&str] = &[
    "entity_kind",
    "release_id",
    "provider_release_id",
    "playlist_id",
    "provider_playlist_id",
    "playlist_provider",
    "playlist_snapshot",
    "playlist_imported_at",
    "retained",
];

pub async fn schema(connection: &mut SqliteConnection) -> Vec<SchemaObject> {
    sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .fetch_all(connection)
        .await
        .unwrap()
}

pub async fn reference_52() -> SqliteConnection {
    let mut reference = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let all = sqlx::migrate!("./migrations");
    for version in [49, 52] {
        let before = schema(&mut reference).await;
        Migrator {
            migrations: Cow::Owned(
                all.iter()
                    .filter(|migration| migration.version <= version)
                    .cloned()
                    .collect(),
            ),
            ..Migrator::DEFAULT
        }
        .run(&mut reference)
        .await
        .unwrap();
        if version == 52 {
            let after = schema(&mut reference).await;
            let mut replaced = Vec::new();
            for object in &before {
                let current = after
                    .iter()
                    .find(|current| current.0 == object.0 && current.1 == object.1)
                    .expect(
                        "the community and collection artwork migrations must not remove prior schema objects",
                    );
                if current != object {
                    replaced.push((object.0.as_str(), object.1.as_str()));
                }
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
            let mut expected_replaced = REPLACED.to_vec();
            let mut expected_added = ADDED.to_vec();
            replaced.sort_unstable();
            added.sort_unstable();
            expected_replaced.sort_unstable();
            expected_added.sort_unstable();
            assert_eq!(
                replaced, expected_replaced,
                "only named artwork schema replacements are allowed"
            );
            assert_eq!(
                added, expected_added,
                "every additive 0050/0051/0052 schema object must be named exactly"
            );
        }
    }
    reference
}

pub async fn reference_53() -> SqliteConnection {
    let mut reference = reference_52().await;
    let before = schema(&mut reference).await;
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 53)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&mut reference)
    .await
    .unwrap();
    let after = schema(&mut reference).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0053 must retain every exact 0052 schema object without replacements"
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
    let mut expected = RANKING_ADDED.to_vec();
    added.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        added, expected,
        "every additive 0053 schema object must be named exactly"
    );
    reference
}

pub fn through(version: i64) -> Migrator {
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

pub fn latest_version() -> i64 {
    sqlx::migrate!("./migrations")
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap()
}

pub fn with_unavailable_outcome(ddl: &str) -> String {
    ddl.replace(
        "'no_suitable_source'",
        "'no_suitable_source','provider_unavailable'",
    )
}

pub async fn reference() -> SqliteConnection {
    identity_schema::reference(reference_53().await).await
}

pub async fn reference_55() -> SqliteConnection {
    let mut reference = reference().await;
    let before = schema(&mut reference).await;
    through(55).run(&mut reference).await.unwrap();
    let expected: Vec<SchemaObject> = before
        .into_iter()
        .map(|mut object| {
            if object.0 == "table" && object.1 == "source_resolutions" {
                object.3 = object.3.map(|ddl| with_unavailable_outcome(&ddl));
            }
            object
        })
        .collect();
    assert!(
        schema(&mut reference).await == expected,
        "0055 must change only the declared source resolution outcome set"
    );
    reference
}

pub async fn reference_56() -> SqliteConnection {
    album_schema::reference(reference_55().await).await
}

pub async fn reference_57() -> SqliteConnection {
    changes_schema::reference(reference_56().await).await
}

pub async fn reference_58() -> SqliteConnection {
    pin_schema::reference(reference_57().await).await
}

pub async fn reference_59() -> SqliteConnection {
    correction_schema::reference(reference_58().await).await
}

pub async fn reference_60() -> SqliteConnection {
    match_schema::reference(reference_59().await).await
}

pub async fn reference_61() -> SqliteConnection {
    provider_evidence_schema::reference(reference_60().await).await
}

pub async fn reference_62() -> SqliteConnection {
    provider_url_schema::reference(reference_61().await).await
}

pub async fn reference_63() -> SqliteConnection {
    name_match_schema::reference(reference_62().await).await
}

pub async fn reference_64() -> SqliteConnection {
    tag_identity_schema::reference(reference_63().await).await
}

pub async fn reference_65() -> SqliteConnection {
    erasure_schema::reference(reference_64().await).await
}

pub async fn reference_66() -> SqliteConnection {
    automation_schema::reference(reference_65().await).await
}

pub async fn reference_67() -> SqliteConnection {
    account_schema::reference(reference_66().await).await
}

pub async fn reference_68() -> SqliteConnection {
    genre_schema::reference(reference_67().await).await
}

pub async fn reference_69() -> SqliteConnection {
    musicbrainz_genres_schema::reference(reference_68().await).await
}

pub async fn reference_70() -> SqliteConnection {
    source_unavailability_schema::reference(reference_69().await).await
}

pub async fn reference_71() -> SqliteConnection {
    credit_matches_schema::reference(reference_70().await).await
}

pub async fn reference_72() -> SqliteConnection {
    name_rule_schema::reference(reference_71().await).await
}

pub async fn reference_73() -> SqliteConnection {
    coverage_matches_schema::reference(reference_72().await).await
}

pub async fn reference_74() -> SqliteConnection {
    name_match_rule_schema::reference(reference_73().await).await
}

pub async fn reference_75() -> SqliteConnection {
    review_queue_schema::reference(reference_74().await).await
}

pub async fn reference_76() -> SqliteConnection {
    recording_id_matches_schema::reference(reference_75().await).await
}

pub async fn reference_77() -> SqliteConnection {
    recording_groups_schema::reference(reference_76().await).await
}

pub async fn reference_78() -> SqliteConnection {
    credited_artists_schema::reference(reference_77().await).await
}

pub async fn reference_79() -> SqliteConnection {
    follow_approval_schema::reference(reference_78().await).await
}

pub async fn reference_80() -> SqliteConnection {
    artist_images_schema::reference(reference_79().await).await
}

pub async fn reference_81() -> SqliteConnection {
    credited_artist_photos_schema::reference(reference_80().await).await
}

pub async fn reference_82() -> SqliteConnection {
    session_client_schema::reference(reference_81().await).await
}

pub async fn reference_83() -> SqliteConnection {
    plugin_authorization_status_schema::reference(reference_82().await).await
}

pub async fn assert_current(connection: &mut SqliteConnection) {
    assert_version(connection, latest_version()).await;
}

pub async fn assert_version(connection: &mut SqliteConnection, version: i64) {
    let mut reference = match version {
        52 => reference_52().await,
        53 => reference_53().await,
        54 => reference().await,
        55 => reference_55().await,
        56 => reference_56().await,
        57 => reference_57().await,
        58 => reference_58().await,
        59 => reference_59().await,
        60 => reference_60().await,
        61 => reference_61().await,
        62 => reference_62().await,
        63 => reference_63().await,
        64 => reference_64().await,
        65 => reference_65().await,
        66 => reference_66().await,
        67 => reference_67().await,
        68 => reference_68().await,
        69 => reference_69().await,
        70 => reference_70().await,
        71 => reference_71().await,
        72 => reference_72().await,
        73 => reference_73().await,
        74 => reference_74().await,
        75 => reference_75().await,
        76 => reference_76().await,
        77 => reference_77().await,
        78 => reference_78().await,
        79 => reference_79().await,
        80 => reference_80().await,
        81 => reference_81().await,
        82 => reference_82().await,
        83 => reference_83().await,
        _ => panic!("The schema fixture requires an exact supported migration endpoint"),
    };
    assert!(
        schema(connection).await == schema(&mut reference).await,
        "the migrated schema must match every declared table, constraint, index, trigger and view exactly"
    );
    for table in [
        "artwork_fetches",
        "recording_artwork",
        "account_community_preferences",
        "release_artwork",
        "playlist_artwork",
        "community_reporters",
        "community_reports",
    ]
    .into_iter()
    .chain((version >= 53).then_some(RANKING_TABLE))
    .chain(
        identity_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 54),
    )
    .chain(
        album_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 56),
    )
    .chain(
        changes_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 57),
    )
    .chain(pin_schema::TABLES.iter().copied().filter(|_| version >= 58))
    .chain(
        correction_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 59),
    )
    .chain(
        match_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 60),
    )
    .chain(
        provider_evidence_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 61),
    )
    .chain(
        provider_url_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 62),
    )
    .chain(
        name_match_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 63),
    )
    .chain(
        tag_identity_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 64),
    )
    .chain(
        erasure_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 65),
    )
    .chain(
        automation_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 66),
    )
    .chain(
        account_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 67),
    )
    .chain(
        genre_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 68),
    )
    .chain(
        musicbrainz_genres_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 69),
    )
    .chain(
        source_unavailability_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 70),
    )
    .chain(
        credit_matches_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 71),
    )
    .chain(
        name_rule_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 72),
    )
    .chain(
        coverage_matches_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 73),
    )
    .chain(
        name_match_rule_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 74),
    )
    .chain(
        review_queue_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 75),
    )
    .chain(
        recording_id_matches_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 76),
    )
    .chain(
        recording_groups_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 77),
    )
    .chain(
        credited_artists_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 78),
    )
    .chain(
        follow_approval_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 79),
    )
    .chain(
        artist_images_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 80),
    )
    .chain(
        credited_artist_photos_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 81),
    )
    .chain(
        session_client_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 82),
    )
    .chain(
        plugin_authorization_status_schema::TABLES
            .iter()
            .copied()
            .filter(|_| version >= 83),
    ) {
        let columns: Vec<Column> = sqlx::query_as("SELECT cid,name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo(?) ORDER BY cid")
            .bind(table).fetch_all(&mut *connection).await.unwrap();
        let expected: Vec<Column> = sqlx::query_as("SELECT cid,name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo(?) ORDER BY cid")
            .bind(table).fetch_all(&mut reference).await.unwrap();
        assert_eq!(
            columns, expected,
            "{table} must retain its exact declared column semantics"
        );
        let foreign_keys: Vec<ForeignKey> = sqlx::query_as("SELECT id,seq,\"table\",\"from\",\"to\",on_update,on_delete,\"match\" FROM pragma_foreign_key_list(?) ORDER BY id,seq")
            .bind(table).fetch_all(&mut *connection).await.unwrap();
        let expected: Vec<ForeignKey> = sqlx::query_as("SELECT id,seq,\"table\",\"from\",\"to\",on_update,on_delete,\"match\" FROM pragma_foreign_key_list(?) ORDER BY id,seq")
            .bind(table).fetch_all(&mut reference).await.unwrap();
        assert_eq!(
            foreign_keys, expected,
            "{table} must retain its exact declared foreign keys"
        );
    }
    for table in NEW_TABLES
        .iter()
        .copied()
        .chain((version >= 53).then_some(RANKING_TABLE))
    {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "historical state must not imply community consent, reporters, contributions or collection covers"
        );
    }
    if version >= 54 {
        identity_schema::assert_empty(connection).await;
    }
    if version >= 56 {
        album_schema::assert_empty(connection).await;
    }
    if version >= 57 {
        changes_schema::assert_empty(connection).await;
    }
    if version >= 58 {
        pin_schema::assert_empty(connection).await;
    }
    if version >= 59 {
        correction_schema::assert_empty(connection).await;
    }
    if version >= 60 {
        match_schema::assert_empty(connection).await;
    }
    if version >= 61 {
        provider_evidence_schema::assert_empty(connection).await;
    }
    if version >= 62 {
        provider_url_schema::assert_empty(connection).await;
    }
    let invented: i64 = sqlx::query_scalar("SELECT count(*) FROM artwork_fetches WHERE entity_kind!='recording'
        OR release_id IS NOT NULL OR provider_release_id IS NOT NULL OR playlist_id IS NOT NULL
        OR provider_playlist_id IS NOT NULL OR playlist_provider IS NOT NULL OR playlist_snapshot IS NOT NULL
        OR playlist_imported_at IS NOT NULL OR retained != EXISTS(SELECT 1 FROM retained_artwork_references r WHERE r.fetch_id=artwork_fetches.id)")
        .fetch_one(&mut *connection).await.unwrap();
    assert_eq!(
        invented, 0,
        "legacy receipts must retain recording identity and exact derived retention"
    );
    let receipt: (i64, i64) =
        sqlx::query_as("SELECT max(version),count(*) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(receipt, (version, version));
    let migrations = sqlx::migrate!("./migrations");
    for version in [50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60]
        .into_iter()
        .filter(|candidate| *candidate <= version)
    {
        let migration = migrations
            .iter()
            .find(|migration| migration.version == version)
            .unwrap();
        let checksum: Vec<u8> = sqlx::query_scalar(
            "SELECT checksum FROM _sqlx_migrations WHERE version=? AND success=1",
        )
        .bind(version)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert_eq!(checksum.as_slice(), migration.checksum.as_ref());
    }
    let ledger: Vec<(i64, String, bool, Vec<u8>)> = sqlx::query_as(
        "SELECT version,description,success,checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    let expected = migrations
        .iter()
        .filter(|migration| migration.version <= version)
        .map(|migration| {
            (
                migration.version,
                migration.description.to_string(),
                true,
                migration.checksum.to_vec(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ledger, expected,
        "every migration identity, success receipt and checksum must match the actual production history"
    );
    reference.close().await.unwrap();
}
