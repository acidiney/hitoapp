#[path = "retirement_fixture.rs"]
mod fixture;

use crate::{
    Action, Actor, Error, Operation, RecordingArtistIdentityState, Services, SyncBatch,
    test_support::work_meter::WorkMeter,
};
use futures_util::FutureExt;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

const SMALL: i64 = 32;
const LARGE: i64 = 100_000;
const FIXED_WORK: u64 = 20_000;

#[tokio::test]
async fn scoped_public_artist_edits_do_not_scan_unrelated_retained_identity_choices() {
    let (fixture, hito, actor) = fixture::Fixture::prepare().await;
    fixture::grow(&hito.catalog.store.pool, &fixture.recording, 1, SMALL + 1).await;
    sqlx::query("ANALYZE")
        .execute(&hito.catalog.store.pool)
        .await
        .unwrap();
    let small = measure(&hito, &actor, &fixture, "small", SMALL).await;
    let template = "a5100000-0000-4000-8000-000000000001";
    fixture::grow(&hito.catalog.store.pool, template, SMALL + 1, LARGE + 1).await;
    hito.catalog.store.pool.close().await;
    drop(hito);
    let mut measured = Vec::new();
    for statistics in ["inherited", "absent", "fresh"] {
        let hito = crate::open(fixture.config()).await.unwrap();
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
        let hito = crate::open(fixture.config()).await.unwrap();
        let actor = hito.identity.authenticate(&fixture.token).await.unwrap();
        fixture.select(&hito, &actor).await;
        measured.push((
            statistics,
            measure(&hito, &actor, &fixture, statistics, LARGE).await,
        ));
        hito.catalog.store.pool.close().await;
    }
    for (statistics, work) in measured {
        assert!(
            work > 0 && work < small + FIXED_WORK,
            "Scoped public artist edit used {work} VM steps with {LARGE} unrelated choices and {statistics} statistics; small={small}, budget={}",
            small + FIXED_WORK
        );
    }
}

async fn measure(
    hito: &Services,
    actor: &Actor,
    fixture: &fixture::Fixture,
    statistics: &str,
    unrelated: i64,
) -> u64 {
    let before = fixture::unrelated_digest(&hito.catalog.store.pool, &fixture.recording).await;
    let revision = hito
        .catalog
        .recording_artist_identity_choice(actor, &fixture.recording)
        .await
        .unwrap()
        .revision;
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let measured = AssertUnwindSafe(meter.measure(hito.collection.apply(
        actor,
        SyncBatch {
            device_id: "identity-retirement-work".into(),
            operations: vec![Operation {
                id: Uuid::new_v4(),
                action: Action::RecordingArtistSet {
                    recording_id: Uuid::parse_str(&fixture.recording).unwrap(),
                    artist: fixture.artist.clone(),
                },
            }],
        },
    )))
    .catch_unwind()
    .await;
    meter.remove(&hito.catalog.store.pool).await;
    let (result, work) = match measured {
        Ok(value) => value,
        Err(panic) => resume_unwind(panic),
    };
    eprintln!(
        "Identity retirement public VM_steps statistics={statistics} unrelated={unrelated} work={work} outcome={}",
        result.as_ref().map_or_else(Error::code, |_| "accepted")
    );
    result.unwrap();
    let choice = hito
        .catalog
        .recording_artist_identity_choice(actor, &fixture.recording)
        .await
        .unwrap();
    assert!(matches!(
        choice.state,
        RecordingArtistIdentityState::NeedsReview(_)
    ));
    assert_eq!(choice.revision, revision + 1);
    let metadata = hito
        .catalog
        .recording_metadata(actor, &fixture.recording, Default::default())
        .await
        .unwrap();
    assert_eq!(
        metadata.selected_observation.unwrap().id,
        fixture.observation
    );
    assert_eq!(
        fixture::unrelated_digest(&hito.catalog.store.pool, &fixture.recording).await,
        before
    );
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM recording_artist_identity_choices WHERE recording_id!=? AND retired=0")
        .bind(&fixture.recording).fetch_one(&hito.catalog.store.pool).await.unwrap();
    assert_eq!(count, unrelated);
    work
}
