use hito_core::test_support::playlist_service as provider;
use hito_core::test_support::support;
#[path = "fixtures/playlist_metadata_upgrade.rs"]
mod upgrade;
#[path = "fixtures/playlist_metadata_upgrade_work.rs"]
mod work;

use sqlx::{Connection, migrate::Migrator};
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

const SMALL_EVENTS: i64 = 6_250;
const LARGE_EVENTS: i64 = 100_000;
const SMALL_RECEIPTS_PER_KIND: i64 = 64;
const LARGE_RECEIPTS_PER_KIND: i64 = 1_024;
const FIXED_WORK: u64 = 500_000;
const WORK_PER_EVENT: u64 = 200;
const WORK_PER_RECEIPT: u64 = 500;

#[tokio::test]
async fn the_metadata_upgrade_keeps_ledger_rebuild_work_bounded_with_ten_accounts_and_receipt_history()
 {
    let seed = work::public_seed().await;
    let large = migrate(&seed, LARGE_EVENTS, LARGE_RECEIPTS_PER_KIND).await;
    let small = migrate(&seed, SMALL_EVENTS, SMALL_RECEIPTS_PER_KIND).await;
    assert!(
        large <= small * 24 + FIXED_WORK,
        "Migration 0046 exceeded its index-growth allowance: small={small}, large={large}"
    );
}

async fn migrate(seed: &work::Seed, events: i64, receipts_per_kind: i64) -> u64 {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let baseline = upgrade::historical_45(&seed.database, &database).await;
    let mut connection = upgrade::connect(&database).await;
    work::grow(&mut connection, &seed.templates, events, receipts_per_kind).await;
    let before = upgrade::checkpoint_45(&mut connection, &baseline).await;
    connection.close().await.unwrap();

    let mut connection = upgrade::connect(&database).await;
    work::assert_population(&mut connection, events, receipts_per_kind).await;
    let receipts = work::ACCOUNTS * receipts_per_kind * 2;
    let budget = FIXED_WORK + events as u64 * WORK_PER_EVENT + receipts as u64 * WORK_PER_RECEIPT;
    let steps = Arc::new(AtomicU64::new(0));
    let exceeded = Arc::new(AtomicBool::new(false));
    let progress = Arc::clone(&steps);
    let interrupted = Arc::clone(&exceeded);
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(1000, move || {
            let current = progress.fetch_add(1000, Ordering::Relaxed) + 1000;
            if current >= budget {
                interrupted.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    let all = sqlx::migrate!("./migrations");
    let measured = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 46)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    let started = Instant::now();
    let result = measured.run(&mut connection).await;
    let elapsed_ms = started.elapsed().as_millis();
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    let work = steps.load(Ordering::Relaxed);
    let aborted = exceeded.load(Ordering::Relaxed);
    if result.is_ok() {
        work::assert_population(&mut connection, events, receipts_per_kind).await;
        upgrade::assert_preserved(&mut connection, &before, 46).await;
    }
    connection.close().await.unwrap();
    eprintln!(
        "Metadata upgrade accounts={} events={events} receipts={receipts} VM_steps~{work} budget={budget} progress_aborted={aborted} elapsed_ms={elapsed_ms}",
        work::ACCOUNTS
    );
    assert!(
        !aborted,
        "Production migration 0046 exceeded its linear work allowance: events={events}, receipts={receipts}, steps~{work}, budget={budget}"
    );
    assert!(
        result.is_ok(),
        "Production migration 0046 failed before its VM budget; this is not the expected work regression"
    );
    assert!(work > 0 && work < budget);
    work
}
