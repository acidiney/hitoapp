#[path = "work_fixture.rs"]
mod fixture;

use crate::test_support::work_meter::WorkMeter;
use crate::{ArtworkFetchStatus, CoreConfig, Error, HostDeviceId, open};
use futures_util::FutureExt;
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use std::{panic::AssertUnwindSafe, path::Path};

const RETAINED: i64 = 100_000;
const SMALL: i64 = 32;
const READ_BUDGET: u64 = 10_000;
const ENQUEUE_BUDGET: u64 = 250_000;
const WORKER_BUDGET: u64 = 500_000;

#[tokio::test]
async fn artwork_work_is_bounded_by_pending_jobs_instead_of_retained_targets_and_album_witnesses() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let database = directory.path().join("seed.db");
    let config = |database: &Path| {
        CoreConfig::new(
            database.to_path_buf(),
            music.clone(),
            HostDeviceId::new("artwork-query-work").unwrap(),
        )
    };
    let provider = fixture::provider::ArtworkServer::start().await;
    let hito = open(config(&database)).await.unwrap();
    let expected = fixture::seed(&hito, &provider).await;
    fixture::grow(&hito, &expected.recording, 1, SMALL).await;
    sqlx::query("ANALYZE")
        .execute(&hito.catalog.store.pool)
        .await
        .unwrap();
    fixture::assert_statistics(&hito, Some((SMALL, SMALL))).await;
    hito.catalog.store.pool.close().await;
    drop(hito);

    let hito = open(config(&database)).await.unwrap();
    fixture::grow(&hito, &expected.recording, SMALL, RETAINED).await;
    fixture::history(&hito, &expected.recording).await;
    fixture::assert_statistics(&hito, Some((SMALL, SMALL))).await;
    fixture::assert_retained(&hito, RETAINED).await;
    hito.catalog.store.pool.close().await;
    drop(hito);
    let mut measurements = Vec::new();
    for statistics in ["inherited", "absent", "fresh"] {
        let copy = directory.path().join(format!("{statistics}.db"));
        std::fs::copy(&database, &copy).unwrap();
        let hito = open(config(&copy)).await.unwrap();
        match statistics {
            "absent" => {
                sqlx::raw_sql("DELETE FROM sqlite_stat1; DELETE FROM sqlite_stat4;")
                    .execute(&hito.catalog.store.pool)
                    .await
                    .unwrap();
            }
            "fresh" => {
                sqlx::query("ANALYZE")
                    .execute(&hito.catalog.store.pool)
                    .await
                    .unwrap();
            }
            _ => {}
        }
        hito.catalog.store.pool.close().await;
        drop(hito);
        let hito = open(config(&copy)).await.unwrap();
        fixture::assert_statistics(
            &hito,
            match statistics {
                "inherited" => Some((SMALL, SMALL)),
                "fresh" => Some((RETAINED, RETAINED + fixture::HISTORY_ROWS)),
                _ => None,
            },
        )
        .await;
        let owner = hito
            .identity
            .authenticate(&expected.owner_token)
            .await
            .unwrap();
        let other = hito
            .identity
            .authenticate(&expected.other_token)
            .await
            .unwrap();
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&hito.catalog.store.pool)
            .await
            .unwrap();
        let stat4: bool = sqlx::query_scalar("SELECT sqlite_compileoption_used('ENABLE_STAT4')")
            .fetch_one(&hito.catalog.store.pool)
            .await
            .unwrap();
        let requests_before = provider.requests().await.len();
        let meter = WorkMeter::install(&hito.catalog.store.pool).await;
        let exercise = AssertUnwindSafe(async {
            let (availability, read_work) = meter.measure(
                hito.catalog.release_artwork_availability(&owner, &expected.release)
            ).await;
            let (enqueue, enqueue_work) = meter.measure(
                hito.catalog.enqueue_release_artwork(&owner, &expected.release)
            ).await;
            let (worker, worker_work) = if enqueue.is_ok() {
                meter.measure(hito.catalog.run_next_artwork_job_until(std::future::pending::<()>())).await
            } else { (Ok(false), 0) };
            eprintln!("Artwork public VM_steps~ sqlite={version} stat4={stat4} statistics={statistics} retained_targets={RETAINED} blobs=1 read={read_work} enqueue={enqueue_work} worker={worker_work} availability={:?} enqueue_result={:?} worker_result={:?}",
                availability.as_ref().map(|value| value.can_fetch).map_err(Error::code),
                enqueue.as_ref().map(|_| "accepted").map_err(Error::code),
                worker.as_ref().map_err(Error::code));
            (availability, enqueue, worker, read_work, enqueue_work, worker_work)
        }).catch_unwind().await;
        meter.remove(&hito.catalog.store.pool).await;
        let (availability, enqueue, worker, read_work, enqueue_work, worker_work) = match exercise {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        };
        fixture::assert_retained(&hito, RETAINED).await;
        assert!(matches!(
            hito.catalog.artwork_fetch(&other, &expected.receipt).await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.catalog.artwork(&other, &expected.recording).await,
            Err(Error::NotFound)
        ));
        let retained = hito
            .catalog
            .artwork(&owner, &expected.recording)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retained.bytes, fixture::provider::RECORDING_COVER);
        if let Ok(receipt) = &enqueue
            && matches!(worker, Ok(true))
        {
            let completed = hito
                .catalog
                .release_artwork_fetch(&owner, &receipt.id)
                .await
                .unwrap();
            assert!(matches!(completed.status, ArtworkFetchStatus::Ready));
            assert_eq!(completed.attempts, 1);
            assert_eq!(
                hito.catalog
                    .release_artwork(&owner, &expected.release)
                    .await
                    .unwrap()
                    .unwrap()
                    .bytes,
                fixture::provider::RELEASE_COVER
            );
            assert_eq!(
                &provider.requests().await[requests_before..],
                &[ArtworkRequest {
                    entity: ArtworkEntity::Release,
                    provider_id: fixture::provider::RELEASE_ID.into(),
                }]
            );
            let unretained: i64 = sqlx::query_scalar("SELECT count(*) FROM artwork_fetches j WHERE status IN ('ready','failed','cancelled') AND NOT EXISTS(SELECT 1 FROM retained_artwork_references retained WHERE retained.fetch_id=j.id)")
                    .fetch_one(&hito.catalog.store.pool).await.unwrap();
            assert_eq!(unretained, 256);
        }
        measurements.push((
            statistics,
            availability,
            enqueue,
            worker,
            read_work,
            enqueue_work,
            worker_work,
        ));
        hito.catalog.store.pool.close().await;
    }
    for (statistics, availability, enqueue, worker, read_work, enqueue_work, worker_work) in
        measurements
    {
        assert!(
            read_work > 0 && read_work < READ_BUDGET,
            "Album availability scanned its 100,000 witnesses with {statistics} statistics: work={read_work}, budget={READ_BUDGET}"
        );
        assert!(availability.unwrap().can_fetch);
        assert!(
            enqueue_work > 0 && enqueue_work < ENQUEUE_BUDGET,
            "Artwork enqueue scanned retained authority or receipts with {statistics} statistics: work={enqueue_work}, budget={ENQUEUE_BUDGET}"
        );
        assert!(enqueue.is_ok(), "authorized album request must be accepted");
        assert!(
            worker_work > 0 && worker_work < WORKER_BUDGET,
            "Artwork claim/finish scanned retained authority or receipts with {statistics} statistics: work={worker_work}, budget={WORKER_BUDGET}"
        );
        assert!(worker.unwrap());
    }
}
