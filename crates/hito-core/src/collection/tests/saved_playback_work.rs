use crate::test_support::work_meter::WorkMeter;
use crate::{
    Action, Actor, CoreConfig, Credentials, Error, HostDeviceId, Operation, RecordingProgress,
    Services, SyncBatch, open,
};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn saved_playback_work_stays_proportional_to_requested_ids_after_catalog_growth() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let config = || {
        CoreConfig::new(
            directory.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("saved-playback-work").unwrap(),
        )
    };
    let hito = open(config()).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "saved-work-owner".into(),
            password: "owned saved playback work password".into(),
        })
        .await
        .unwrap();
    let owner = hito.identity.authenticate(&session.token).await.unwrap();
    let library = hito.catalog.libraries(&owner).await.unwrap().remove(0).id;
    insert_recordings(&hito, &owner, &library, 0, 1, false).await;
    save_progress(&hito, &owner, expected_progress(0)).await;
    sqlx::query("ANALYZE")
        .execute(&hito.collection.store.pool)
        .await
        .unwrap();
    let inherited_stat: String = sqlx::query_scalar(
        "SELECT stat FROM sqlite_stat1 WHERE tbl='recordings' AND idx='recordings_library'",
    )
    .fetch_one(&hito.collection.store.pool)
    .await
    .unwrap();
    assert_eq!(inherited_stat.split_whitespace().next(), Some("1"));
    hito.collection.store.pool.close().await;
    drop(hito);

    let hito = open(config()).await.unwrap();
    let owner = hito.identity.authenticate(&session.token).await.unwrap();
    insert_recordings(&hito, &owner, &library, 1, 100_000, true).await;
    let foreign_credentials = || Credentials {
        username: "saved-work-other".into(),
        password: "other private saved playback work password".into(),
    };
    hito.identity
        .create_user(&owner, foreign_credentials())
        .await
        .unwrap();
    let foreign_session = hito.identity.login(foreign_credentials()).await.unwrap();
    let foreign = hito
        .identity
        .authenticate(&foreign_session.token)
        .await
        .unwrap();
    let mut tx = hito.collection.store.pool.begin().await.unwrap();
    let foreign_library = crate::identity::personal_library(&mut tx, foreign.id())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    insert_recordings(&hito, &foreign, &foreign_library, 100_000, 100_001, false).await;
    let foreign_progress = RecordingProgress {
        recording_id: recording_id(100_000),
        position_ms: 333,
        speed: 1.25,
    };
    save_progress(&hito, &foreign, foreign_progress).await;
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM recordings WHERE library_id=?),
            (SELECT count(*) FROM listening_progress WHERE user_id=?),
            (SELECT count(*) FROM account_recording_credits WHERE user_id=?)",
    )
    .bind(&library)
    .bind(owner.id())
    .bind(owner.id())
    .fetch_one(&hito.collection.store.pool)
    .await
    .unwrap();
    assert_eq!(counts, (100_000, 100_000, 100_000));
    let retained_stat: String = sqlx::query_scalar(
        "SELECT stat FROM sqlite_stat1 WHERE tbl='recordings' AND idx='recordings_library'",
    )
    .fetch_one(&hito.collection.store.pool)
    .await
    .unwrap();
    assert_eq!(retained_stat, inherited_stat);
    let foreign_key_errors: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&hito.collection.store.pool)
            .await
            .unwrap();
    assert!(foreign_key_errors.is_empty());
    hito.collection.store.pool.close().await;
    drop(hito);

    let mut measurements = Vec::new();
    let mut privacy_measurements = Vec::new();
    for statistics in ["inherited", "absent", "fresh"] {
        let hito = open(config()).await.unwrap();
        match statistics {
            "absent" => {
                sqlx::raw_sql("DELETE FROM sqlite_stat1; DELETE FROM sqlite_stat4;")
                    .execute(&hito.collection.store.pool)
                    .await
                    .unwrap();
            }
            "fresh" => {
                sqlx::query("ANALYZE")
                    .execute(&hito.collection.store.pool)
                    .await
                    .unwrap();
            }
            _ => {}
        }
        hito.collection.store.pool.close().await;
        drop(hito);
        let hito = open(config()).await.unwrap();
        let owner = hito.identity.authenticate(&session.token).await.unwrap();
        let foreign = hito
            .identity
            .authenticate(&foreign_session.token)
            .await
            .unwrap();
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&hito.collection.store.pool)
            .await
            .unwrap();
        let stat4: bool = sqlx::query_scalar("SELECT sqlite_compileoption_used('ENABLE_STAT4')")
            .fetch_one(&hito.collection.store.pool)
            .await
            .unwrap();
        let meter = WorkMeter::install(&hito.collection.store.pool).await;
        let exercise = AssertUnwindSafe(async {
            for indices in [vec![99_999], (0..200).map(|i| 99_999 - i * 499).collect()] {
                let requested: Vec<Uuid> = indices.iter().map(|i| recording_id(*i)).collect();
                let expected: Vec<RecordingProgress> = indices.into_iter().map(expected_progress).collect();
                let (result, work) = meter.measure(hito.collection.saved_playback(&owner, &requested)).await;
                eprintln!(
                    "Saved playback public VM_steps~ sqlite={version} stat4={stat4} \
                     statistics={statistics} owned_records=100000 requested={} work={work} outcome={:?}",
                    requested.len(), result.as_ref().map(Vec::len).map_err(Error::code),
                );
                measurements.push((statistics, expected, result, work));
            }
            for (actor, expected) in [(&owner, None), (&foreign, Some(foreign_progress))] {
                let (result, work) = meter.measure(hito.collection.saved_playback(
                    actor, &[recording_id(100_000)],
                )).await;
                privacy_measurements.push((statistics, expected, result, work));
            }
            let (mixed, work) = meter.measure(hito.collection.saved_playback(
                &owner, &[recording_id(99_999), recording_id(100_000)],
            )).await;
            privacy_measurements.push((statistics, None, mixed, work));
        }).catch_unwind().await;
        meter.remove(&hito.collection.store.pool).await;
        hito.collection.store.pool.close().await;
        if let Err(panic) = exercise {
            std::panic::resume_unwind(panic);
        }
    }
    for (statistics, expected, result, work) in measurements {
        let budget = 2_000 + expected.len() as u64 * 200;
        if expected.len() == 200 {
            assert!(work > 0);
        }
        assert!(
            work < budget,
            "Public saved playback used approximately {work} VM steps with {statistics} statistics \
             for {} requested IDs among 100,000 owned recordings; budget={budget}",
            expected.len()
        );
        assert_eq!(
            result.expect("authorized progress read must succeed"),
            expected
        );
    }
    for (statistics, expected, result, work) in privacy_measurements {
        assert!(
            work < 2_400,
            "Private saved playback used {work} VM steps with {statistics} statistics"
        );
        match expected {
            Some(expected) => assert_eq!(result.unwrap(), vec![expected]),
            None => assert!(matches!(result, Err(Error::NotFound))),
        }
    }
}

async fn insert_recordings(
    hito: &Services,
    actor: &Actor,
    library: &str,
    start: i64,
    end: i64,
    with_progress: bool,
) {
    let mut tx = hito.collection.store.pool.begin().await.unwrap();
    let foreign_keys: bool = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(foreign_keys);
    let records = sqlx::query(
        "WITH RECURSIVE fixture(i) AS (
            VALUES(?) UNION ALL SELECT i+1 FROM fixture WHERE i+1 < ?
         ) INSERT INTO recordings(id,library_id,title,artist,duration_ms,provenance)
         SELECT printf('b1000000-0000-4000-8000-%012x',i), ?,
            printf('Owned playback %06d',i),'Owned playback author',1000,'file_tags'
         FROM fixture",
    )
    .bind(start)
    .bind(end)
    .bind(library)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(records.rows_affected(), (end - start) as u64);
    crate::metadata::provision(&mut tx, actor.id(), library)
        .await
        .unwrap();
    if with_progress {
        let progress = sqlx::query(
            "WITH RECURSIVE fixture(i) AS (
                VALUES(?) UNION ALL SELECT i+1 FROM fixture WHERE i+1 < ?
             ) INSERT INTO listening_progress(user_id,recording_id,position_ms,speed)
             SELECT ?,printf('b1000000-0000-4000-8000-%012x',i),31+i*7,0.5+(i%7)*0.25
             FROM fixture",
        )
        .bind(start)
        .bind(end)
        .bind(actor.id())
        .execute(&mut *tx)
        .await
        .unwrap();
        assert_eq!(progress.rows_affected(), (end - start) as u64);
    }
    tx.commit().await.unwrap();
}

async fn save_progress(hito: &Services, actor: &Actor, progress: RecordingProgress) {
    hito.collection
        .apply(
            actor,
            SyncBatch {
                device_id: "saved-playback-work".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::ProgressSet {
                        recording_id: progress.recording_id,
                        position_ms: progress.position_ms as i64,
                        speed: progress.speed,
                    },
                }],
            },
        )
        .await
        .unwrap();
}

fn expected_progress(index: i64) -> RecordingProgress {
    RecordingProgress {
        recording_id: recording_id(index),
        position_ms: (31 + index * 7) as u64,
        speed: 0.5 + (index % 7) as f64 * 0.25,
    }
}

fn recording_id(index: i64) -> Uuid {
    Uuid::parse_str(&format!("b1000000-0000-4000-8000-{index:012x}")).unwrap()
}
