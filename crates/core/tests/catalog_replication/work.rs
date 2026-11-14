#[path = "work_fixture.rs"]
mod fixture;

#[path = "construction_stage_diagnostic.rs"]
mod diagnostic;

#[path = "materialization_batch_boundary.rs"]
mod materialization_batch_boundary;
#[path = "materialization_batch_boundary_fixture.rs"]
mod materialization_batch_boundary_fixture;
#[path = "materialization_limits.rs"]
mod materialization_limits;

use crate::test_support::support;
use fixture::{install_meter, populate};
use serde_json::Value;
use std::sync::atomic::Ordering;
use tracing::instrument::WithSubscriber;

fn summary(report: &Value) -> &Value {
    let events = report["events"].as_array().unwrap();
    let summaries: Vec<_> = events
        .iter()
        .filter(|event| event["stage"] == "materialization_summary")
        .collect();
    assert_eq!(summaries.len(), 1);
    summaries[0]
}

#[test]
fn materialization_diagnostic_reports_success_and_error_once() {
    for success in [true, false] {
        let diagnostics = diagnostic::ConstructionStages::new(std::time::Instant::now());
        let dispatch = diagnostics.dispatch();
        tracing::dispatcher::with_default(&dispatch, || {
            let mut materialization = super::materialization::MaterializationDiagnostic::new();
            materialization.start(super::materialization::Phase::RecordingBatchQuery);
            materialization.end();
            materialization.recording_batch_finished();
            materialization.recording_finished();
            let outcome = if success {
                Ok(())
            } else {
                Err(crate::Error::Internal("diagnostic control".into()))
            };
            materialization.finish(&outcome);
            drop(materialization);
        });
        let report = diagnostics.report();
        let event = summary(&report);
        assert_eq!(event["completed"], success);
        assert_eq!(event["outcome_code"], if success { 1 } else { 2 });
        assert_eq!(event["completed_batches"], 1);
        assert_eq!(event["completed_recordings"], 1);
        assert_eq!(event["recording_batch_query_calls"], 1);
        assert_eq!(event["active_phase"], 0);
        assert!(!report["overflowed"].as_bool().unwrap());
    }
}

#[test]
fn materialization_diagnostic_reports_a_cancelled_pending_phase_once() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };

    let diagnostics = diagnostic::ConstructionStages::new(std::time::Instant::now());
    let dispatch = diagnostics.dispatch();
    tracing::dispatcher::with_default(&dispatch, || {
        let mut pending = Box::pin(async {
            let mut materialization = super::materialization::MaterializationDiagnostic::new();
            materialization.recording_finished();
            materialization.start(super::materialization::Phase::SourceFactsQuery);
            std::future::pending::<()>().await;
            materialization.finish(&Ok(()));
        });
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(pending.as_mut().poll(&mut context), Poll::Pending));
        drop(pending);
    });
    let report = diagnostics.report();
    let event = summary(&report);
    assert_eq!(event["completed"], false);
    assert_eq!(event["outcome_code"], 0);
    assert_eq!(event["active_phase"], 5);
    assert_eq!(event["completed_recordings"], 1);
    assert!(event["active_partial_us"].as_u64().is_some());
    assert!(!report["overflowed"].as_bool().unwrap());
}

#[tokio::test]
async fn public_snapshot_sql_work_grows_with_catalog_size() {
    for statistics in ["absent", "stale", "fresh"] {
        let mut measured = Vec::new();
        for rows in [128, 512] {
            let (_dir, services, actor, library, music) = support::library("catalog-work").await;
            support::write_wav(&music.join("owned.wav"), 0);
            support::scan(&services, &actor, &library).await;
            populate(&services, &library, rows, statistics).await;
            let pool = &services.catalog_replication.store.pool;
            {
                let mut idle = services.catalog_replication.reader.lock().await;
                super::materialization::explain_plans(
                    idle.as_mut().unwrap(),
                    &actor,
                    &services.catalog_replication.host,
                    &library,
                )
                .await;
            }
            let steps = install_meter(&services).await;
            let started = std::time::Instant::now();
            let snapshot = services
                .catalog_replication
                .begin(&actor, &library)
                .await
                .unwrap();
            let work = steps.load(Ordering::Relaxed);
            eprintln!(
                "statistics={statistics} catalog_rows={rows} sqlite_vm_steps~{work} elapsed={:?}",
                started.elapsed()
            );
            assert_eq!(snapshot.recording_count, rows);
            assert!(
                work > 0 && work < 100_000_000,
                "snapshot exceeded a fixed VM-work budget: {work}"
            );
            measured.push(work);
            services.catalog_replication.close().await.unwrap();
            pool.close().await;
        }
        assert!(
            measured[1] <= measured[0] * 6 + 100_000,
            "4x catalog growth must stay within a linear work allowance: statistics={statistics}, small={}, large={}",
            measured[0],
            measured[1]
        );
    }
}

#[tokio::test]
async fn one_hundred_thousand_local_history_facts_fit_the_snapshot_budget() {
    for statistics in ["absent", "stale", "fresh"] {
        let (_dir, services, actor, library, music) = support::library("catalog-scale").await;
        support::write_wav(&music.join("owned.wav"), 0);
        support::scan(&services, &actor, &library).await;
        populate(&services, &library, 100_000, statistics).await;
        {
            let mut idle = services.catalog_replication.reader.lock().await;
            super::materialization::explain_plans(
                idle.as_mut().unwrap(),
                &actor,
                &services.catalog_replication.host,
                &library,
            )
            .await;
        }
        let steps = install_meter(&services).await;
        let started = std::time::Instant::now();
        let diagnostics = diagnostic::ConstructionStages::new(started);
        let outcome = services
            .catalog_replication
            .begin(&actor, &library)
            .with_subscriber(diagnostics.dispatch())
            .await;
        let elapsed = started.elapsed();
        let work = steps.load(Ordering::Relaxed);
        let error_class = match &outcome {
            Ok(_) => "success",
            Err(crate::Error::Upstream("Catalog snapshot construction timed out")) => {
                "construction_timeout"
            }
            Err(_) => "other_error",
        };
        eprintln!(
            "catalog_snapshot_stage_diagnostic statistics={statistics} error_class={error_class} stages={}",
            diagnostics.report()
        );
        let snapshot = outcome.unwrap_or_else(|error| {
            panic!(
                "100k snapshot construction failed: statistics={statistics} error={error:?} elapsed={elapsed:?} sqlite_vm_steps~{work}"
            )
        });
        assert_eq!(snapshot.recording_count, 100_000);
        assert!(
            work > 0 && work < 60_000_000,
            "100k snapshot exceeded its VM-work budget: {work}"
        );
        let mut identities = std::collections::HashSet::new();
        let mut bytes = 0;
        let mut ready = 0;
        for ordinal in 0..snapshot.page_count {
            let page = services
                .catalog_replication
                .page(&actor, snapshot.id, ordinal)
                .await
                .unwrap();
            bytes += serde_json::to_vec(&page).unwrap().len();
            for fact in page.recordings {
                assert!(identities.insert(fact.id));
                ready += fact
                    .sources
                    .iter()
                    .filter(|source| source.server_relay == crate::ServerRelayObservation::Ready)
                    .count();
            }
        }
        assert_eq!(identities.len(), 100_000);
        assert_eq!(
            ready, 1,
            "only the actual owned WAV has a ready relay observation"
        );
        eprintln!(
            "statistics={statistics} catalog_rows=100000 sqlite_vm_steps~{work} construction={elapsed:?} canonical_bytes={bytes} pages={}",
            snapshot.page_count
        );
        services.catalog_replication.close().await.unwrap();
        services.catalog_replication.store.pool.close().await;
    }
}

#[tokio::test]
async fn stale_statistics_do_not_multiply_work_by_release_and_file_counts() {
    let mut measured = Vec::new();
    for rows in [128, 512] {
        let (_dir, services, actor, library, music) =
            support::library("catalog-stale-coordinates").await;
        support::write_wav(&music.join("owned.wav"), 0);
        support::scan(&services, &actor, &library).await;
        populate(&services, &library, rows, "stale").await;
        {
            let mut idle = services.catalog_replication.reader.lock().await;
            super::materialization::explain_plans(
                idle.as_mut().unwrap(),
                &actor,
                &services.catalog_replication.host,
                &library,
            )
            .await;
        }
        let steps = install_meter(&services).await;
        let snapshot = services
            .catalog_replication
            .begin(&actor, &library)
            .await
            .unwrap();
        let work = steps.load(Ordering::Relaxed);
        assert_eq!(snapshot.recording_count, rows);
        eprintln!("stale_distinct_releases_and_files={rows} sqlite_vm_steps~{work}");
        measured.push(work);
        services.catalog_replication.close().await.unwrap();
        services.catalog_replication.store.pool.close().await;
    }
    assert!(
        measured[1] <= measured[0] * 6 + 100_000,
        "4x growth in recordings, releases and files must stay linear: small={}, large={}",
        measured[0],
        measured[1]
    );
}
