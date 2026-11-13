#[path = "artist_credit_target_upgrade_history.rs"]
mod historical;
#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;

use futures_util::FutureExt;
use hito_core::test_support::{database_owner::DatabaseOwner, hang_guard::hang_guard};
use hito_core::{CoreConfig, HostDeviceId, RecordingArtistIdentityState};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{borrow::Cow, panic::AssertUnwindSafe, path::Path, sync::Arc, time::Duration};
use tokio::time::timeout;

const RECORDING: &str = "00000000-0000-4000-8000-000000000001";
const ASSOCIATION: &str = "00000000-0000-4000-8000-000000000002";
const GROUP: &str = "00000000-0000-4000-8000-000000000003";
const SESSION: &str = "owned-credit-upgrade-session";

const CHILDREN: &[(&str, i64)] = &[
    ("recording_artist_identity_choices", 1),
    ("album_edition_choices", 1),
    ("recording_artist_association_changes", 5),
    ("artist_identity_change_items", 2),
    ("artist_association_change_retirements", 1),
    ("shared_artist_corrections", 1),
    ("artist_association_matches", 1),
    ("artist_association_provider_matches", 1),
    ("artist_association_name_matches", 1),
    ("artist_association_credit_matches", 1),
    ("artist_association_coverage_matches", 1),
    ("artist_association_candidate_confirmations", 1),
    ("artist_association_recording_id_matches", 1),
];

pub(super) struct Prepared {
    rows: Vec<snapshot::Projection>,
    history: historical::History,
    generated_keys: Vec<(i64, String, String)>,
}

#[tokio::test]
async fn opening_schema_95_preserves_the_existing_artist_choice_and_all_prior_rows() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let database = directory.path().join("hito.db");
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let before = Box::pin(prepare(&database)).await;
    let Prepared {
        rows,
        history: commands,
        generated_keys,
    } = before;
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        database.clone(),
        music,
        HostDeviceId::new("credit-upgrade").unwrap(),
    );
    owner.attach(&mut config);
    let mut services = None;
    let outcome = AssertUnwindSafe(Box::pin(timeout(Duration::from_secs(30), async {
        services = Some(hito_core::open(config).await.unwrap());
        let mut inspector = connect(&database).await;
        let migrated =
            AssertUnwindSafe(Box::pin(snapshot::assert_preserved(&mut inspector, &rows)))
                .catch_unwind()
                .await;
        inspector.close().await.unwrap();
        if let Err(panic) = migrated {
            std::panic::resume_unwind(panic);
        }
        let hito = services.as_ref().unwrap();
        let actor = hito.identity.authenticate(SESSION).await.unwrap();
        let choice = hito
            .artist_catalog
            .recording_artist_identity_choice(&actor, RECORDING)
            .await
            .unwrap();
        assert_eq!(choice.revision, 3);
        assert!(
            matches!(&choice.state, RecordingArtistIdentityState::Selected(selection)
            if selection.association_id == ASSOCIATION && selection.group_id == GROUP)
        );
        let associations = hito
            .artist_catalog
            .recording_artist_associations(&actor, RECORDING)
            .await
            .unwrap();
        assert_eq!(associations.len(), 2);
        assert!(
            associations
                .iter()
                .any(|association| association.id == ASSOCIATION)
        );
        let current_choice = serde_json::to_value(choice).unwrap();
        Box::pin(commands.replay(hito, &actor)).await;
        assert_eq!(
            serde_json::to_value(
                hito.artist_catalog
                    .recording_artist_identity_choice(&actor, RECORDING)
                    .await
                    .unwrap()
            )
            .unwrap(),
            current_choice
        );
    })))
    .catch_unwind()
    .await;
    if let Some(hito) = services.take() {
        hito.playback.close().await.unwrap();
        hito.replicas.close().await.unwrap();
        hito.catalog_replication.close().await.unwrap();
        drop(hito);
    }
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(_)) => panic!("the public schema upgrade exceeded its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    let mut connection = connect(&database).await;
    let preserved = AssertUnwindSafe(Box::pin(async {
        snapshot::assert_preserved(&mut connection, &rows).await;
        assert_children(&mut connection).await;
        assert_eq!(keys(&mut connection).await, generated_keys);
        let history: Vec<i64> = sqlx::query_scalar(
            "SELECT version FROM _sqlx_migrations WHERE success=1 ORDER BY version",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert_eq!(history, (1..=97).collect::<Vec<_>>());
        let targets: Vec<String> = sqlx::query_scalar(
            "SELECT credit_target FROM recording_artist_associations ORDER BY id",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert_eq!(targets, ["whole_credit", "whole_credit", "whole_credit"]);
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut connection)
                .await
                .unwrap()
                .is_empty()
        );
        let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_all(&mut connection)
            .await
            .unwrap();
        assert_eq!(integrity, ["ok"]);
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(foreign_keys, 1);
    }))
    .catch_unwind()
    .await;
    connection.close().await.unwrap();
    if let Err(panic) = preserved {
        std::panic::resume_unwind(panic);
    }
}

async fn connect(database: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

pub(super) async fn prepare(database: &Path) -> Prepared {
    let mut connection = None;
    let outcome = AssertUnwindSafe(Box::pin(hang_guard(
        "the populated schema 95 must be prepared",
        Box::pin(async {
            connection = Some(connect(database).await);
            let connection = connection.as_mut().unwrap();
            let all = hito_core::test_support::migrations();
            Migrator {
                migrations: Cow::Owned(
                    all.iter()
                        .filter(|migration| migration.version <= 95)
                        .cloned()
                        .collect(),
                ),
                ..Migrator::DEFAULT
            }
            .run(&mut *connection)
            .await
            .unwrap();
            sqlx::raw_sql(include_str!("artist_credit_target_upgrade.sql"))
                .execute(&mut *connection)
                .await
                .unwrap();
            let history = historical::History::seed(&mut *connection).await;
            sqlx::raw_sql(include_str!("artist_credit_target_upgrade_history.sql"))
                .execute(&mut *connection).await.unwrap();
            assert_children(&mut *connection).await;
            let generated_keys = keys(&mut *connection).await;
            assert_eq!(generated_keys.len(), 3);
            assert_eq!(generated_keys.iter().filter(|(_, _, key)| key == "builtin:musicbrainz").count(), 2);
            assert_eq!(generated_keys.iter().filter(|(_, _, key)| key == "http:00000000-0000-4000-8000-000000000061").count(), 1);
            sqlx::query(
                "INSERT INTO sessions(token_hash,user_id,expires_at,created_at,last_seen_at)
                 VALUES(?, '00000000-0000-4000-8000-000000000010',4102444800,unixepoch(),unixepoch()),
                     (?, '00000000-0000-4000-8000-000000000010',4102444800,0,0)",
            )
            .bind(format!("{:x}", Sha256::digest(SESSION.as_bytes())))
            .bind(format!("{:x}", Sha256::digest(b"owned-unauthed-historical-session")))
            .execute(&mut *connection)
            .await
            .unwrap();
            assert!(
                sqlx::query("PRAGMA foreign_key_check")
                    .fetch_all(&mut *connection)
                    .await
                    .unwrap()
                    .is_empty()
            );
            let rows = snapshot::capture(&mut *connection, 95).await;
            Prepared { rows, history, generated_keys }
        }),
    )))
    .catch_unwind()
    .await;
    if let Some(connection) = connection {
        connection.close().await.unwrap();
    }
    match outcome {
        Ok(prepared) => prepared,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

async fn assert_children(connection: &mut SqliteConnection) {
    assert_eq!(CHILDREN.len(), 13);
    for &(table, expected) in CHILDREN {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(
            count, expected,
            "the owned reference table must retain every seeded row: {table}"
        );
    }
    for (table, expected) in [
        ("artist_identity_changes", 2),
        ("shared_artist_correction_subjects", 1),
        ("shared_artist_correction_states", 2),
        ("artist_identity_choice_receipts", 3),
        ("recording_artist_association_receipts", 2),
        ("album_edition_choice_receipts", 1),
        ("shared_artist_correction_receipts", 2),
        ("recording_artist_association_change_order", 5),
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(
            count, expected,
            "the owned history and receipt table must retain every seeded row: {table}"
        );
    }
}

async fn keys(connection: &mut SqliteConnection) -> Vec<(i64, String, String)> {
    sqlx::query_as("SELECT rowid,id,provider_key FROM recording_artist_associations ORDER BY rowid")
        .fetch_all(connection)
        .await
        .unwrap()
}
