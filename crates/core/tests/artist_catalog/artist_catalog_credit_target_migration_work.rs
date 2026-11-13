#[path = "artist_credit_target_work_receipts.rs"]
mod receipts;
#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;

use super::artist_catalog_credit_target_upgrade as upgrade;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{
    borrow::Cow,
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use uuid::Uuid;

const RECORDING: &str = "00000000-0000-4000-8000-000000000081";
const RELEASE: &str = "00000000-0000-4000-8000-000000000082";
const WORK_CEILING: u64 = 10_000_000;
const CHILDREN: &[&str] = &[
    "recording_artist_identity_choices",
    "album_edition_choices",
    "recording_artist_association_changes",
    "artist_identity_change_items",
    "artist_association_change_retirements",
    "shared_artist_corrections",
    "artist_association_matches",
    "artist_association_provider_matches",
    "artist_association_name_matches",
    "artist_association_credit_matches",
    "artist_association_coverage_matches",
    "artist_association_candidate_confirmations",
    "artist_association_recording_id_matches",
];

struct Sample {
    accounts: usize,
    associations: i64,
    child_rows: i64,
    instructions: u64,
    completed: bool,
}

#[tokio::test]
async fn migration_96_work_is_bounded_for_growing_account_scoped_artist_histories() {
    let mut previous: Option<Sample> = None;
    for accounts in [16, 64, 256] {
        let sample = Box::pin(measure(accounts)).await;
        println!(
            "migration_work accounts={} associations={} child_rows={} instructions={} completed={}",
            sample.accounts,
            sample.associations,
            sample.child_rows,
            sample.instructions,
            sample.completed
        );
        assert!(
            sample.instructions > 0,
            "the actual migration must produce observable SQLite instruction work"
        );
        assert!(
            sample.instructions < WORK_CEILING,
            "migration96 exceeded its fixed instruction ceiling after owned cleanup: accounts={}, instructions={}, ceiling={WORK_CEILING}",
            sample.accounts,
            sample.instructions
        );
        assert!(
            sample.completed,
            "migration96 must complete below its instruction ceiling"
        );
        if let Some(previous) = previous {
            assert_eq!(sample.accounts, previous.accounts * 4);
            let bound = previous
                .instructions
                .saturating_mul(5)
                .saturating_add(100_000);
            assert!(
                sample.instructions <= bound,
                "migration96 work grew faster than its fixed scaling bound after owned cleanup: accounts={} to {}, instructions={} to {}, bound={bound}",
                previous.accounts,
                sample.accounts,
                previous.instructions,
                sample.instructions
            );
        }
        previous = Some(sample);
    }
}

async fn measure(accounts: usize) -> Sample {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("artist-history-work.db");
    drop(Box::pin(upgrade::prepare(&database)).await);
    let mut connection = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let before = hang_guard("the scaled historical artist graph must be prepared", Box::pin(async {
            connection = Some(SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database).create_if_missing(false).foreign_keys(true)).await.unwrap());
            let connection = connection.as_mut().unwrap();
            sqlx::raw_sql(include_str!("artist_credit_target_work_shared_library.sql")).execute(&mut *connection).await.unwrap();
            for index in 1..=accounts {
                let account = Account::new(index);
                let mut transaction = connection.begin().await.unwrap();
                sqlx::query("INSERT INTO users(id,username,password_hash,admin) VALUES(?,?, 'owned-unused-work-hash',1)")
                    .bind(&account.owner).bind(format!("owned-work-{}", account.owner)).execute(&mut *transaction).await.unwrap();
                sqlx::query("INSERT INTO library_members(library_id,user_id) VALUES('owned-work-library',?)")
                    .bind(&account.owner).execute(&mut *transaction).await.unwrap();
                receipts::seed(&mut transaction, &account).await;
                sqlx::raw_sql(&account.sql()).execute(&mut *transaction).await.unwrap();
                transaction.commit().await.unwrap();
            }
            assert_eq!(CHILDREN.len(), 13);
            let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&mut *connection).await.unwrap();
            assert_eq!(owners, accounts as i64 + 1);
            assert!(sqlx::query("PRAGMA foreign_key_check").fetch_all(&mut *connection).await.unwrap().is_empty());
            snapshot::capture(&mut *connection, 95).await
        })).await;
        let connection = connection.as_mut().unwrap();
        let associations: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_artist_associations").fetch_one(&mut *connection).await.unwrap();
        assert_eq!(associations, (2 * accounts + 3) as i64);
        let mut child_rows = 0;
        for table in CHILDREN {
            let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}")).fetch_one(&mut *connection).await.unwrap();
            assert!(rows >= accounts as i64);
            child_rows += rows;
        }
        assert_eq!(child_rows, (17 * accounts + 18) as i64);
        let keys: Vec<(i64, String, String)> = sqlx::query_as("SELECT rowid,id,provider_key FROM recording_artist_associations ORDER BY rowid").fetch_all(&mut *connection).await.unwrap();
        let through_96 = Migrator {
            migrations: Cow::Owned(hito_core::test_support::migrations().iter().filter(|migration| migration.version <= 96).cloned().collect()),
            ..Migrator::DEFAULT
        };
        let steps = Arc::new(AtomicU64::new(0));
        let progress = Arc::clone(&steps);
        connection.lock_handle().await.unwrap().set_progress_handler(100, move || progress.fetch_add(100, Ordering::Relaxed) + 100 < WORK_CEILING);
        let migrated = through_96.run(&mut *connection).await;
        connection.lock_handle().await.unwrap().remove_progress_handler();
        let instructions = steps.load(Ordering::Relaxed);
        if instructions < WORK_CEILING {
            assert!(migrated.is_ok(), "migration96 failed before its work ceiling: {migrated:?}");
        }
        let completed = migrated.is_ok();
        if completed {
            let targeted: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_artist_associations WHERE credit_target='whole_credit'").fetch_one(&mut *connection).await.unwrap();
            assert_eq!(targeted, associations);
        }
        snapshot::assert_preserved(&mut *connection, &before).await;
        let retained_keys: Vec<(i64, String, String)> = sqlx::query_as("SELECT rowid,id,provider_key FROM recording_artist_associations ORDER BY rowid").fetch_all(&mut *connection).await.unwrap();
        assert_eq!(retained_keys, keys);
        assert!(sqlx::query("PRAGMA foreign_key_check").fetch_all(&mut *connection).await.unwrap().is_empty());
        let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check").fetch_all(&mut *connection).await.unwrap();
        assert_eq!(integrity, ["ok"]);
        let version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations").fetch_one(&mut *connection).await.unwrap();
        assert_eq!(version, if completed { 96 } else { 95 });
        Sample { accounts, associations, child_rows, instructions, completed }
    })).catch_unwind().await;
    if let Some(mut connection) = connection {
        connection
            .lock_handle()
            .await
            .unwrap()
            .remove_progress_handler();
        connection.close().await.unwrap();
    }
    match outcome {
        Ok(sample) => sample,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

struct Account {
    owner: String,
    association: String,
    retired: String,
    group: String,
    split: String,
    restore: String,
    correction: String,
    sequence: i64,
    chosen_order: i64,
    split_order: i64,
    restore_order: i64,
    change_floor: i64,
}

impl Account {
    fn new(index: usize) -> Self {
        let id = |kind: u128| {
            Uuid::from_u128(0x00000000_0000_4000_8000_000000000000 | ((index as u128) << 16) | kind)
                .to_string()
        };
        let chosen_order = 1000 + 3 * index as i64;
        Self {
            owner: id(1),
            association: id(2),
            retired: id(3),
            group: id(4),
            split: id(5),
            restore: id(6),
            correction: id(7),
            sequence: index as i64 + 1,
            chosen_order,
            split_order: chosen_order + 1,
            restore_order: chosen_order + 2,
            change_floor: 4 * index as i64 + 4,
        }
    }

    fn sql(&self) -> String {
        let mut sql = include_str!("artist_credit_target_work_account.sql").to_string();
        for (token, value) in [
            ("@OWNER@", self.owner.clone()),
            ("@ASSOCIATION@", self.association.clone()),
            ("@RETIRED@", self.retired.clone()),
            ("@GROUP@", self.group.clone()),
            ("@SPLIT@", self.split.clone()),
            ("@RESTORE@", self.restore.clone()),
            ("@CORRECTION@", self.correction.clone()),
            ("@SEQUENCE@", self.sequence.to_string()),
            ("@CHOSEN_ORDER@", self.chosen_order.to_string()),
            ("@SPLIT_ORDER@", self.split_order.to_string()),
            ("@RESTORE_ORDER@", self.restore_order.to_string()),
            ("@CHANGE_FLOOR@", self.change_floor.to_string()),
        ] {
            sql = sql.replace(token, &value);
        }
        assert!(!sql.contains('@'));
        sql
    }
}
