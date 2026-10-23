use crate::test_support::work_meter::WorkMeter;
use crate::{
    Browse, CoreConfig, Credentials, HostDeviceId, PodcastEpisodeQuery, SubscribePodcast, open,
};
use futures_util::FutureExt;
use sqlx::Connection;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

const ARCHIVE_WORK_BUDGET: u64 = 50_000;
const PAGE_SIZE: u16 = 20;

#[tokio::test]
async fn podcast_episode_pages_stay_bounded_across_a_large_retained_archive() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("podcast-archive-work").unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-archive-owner".into(),
            password: "owned podcast archive password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let initial = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: Uuid::new_v4(),
                feed_url: "https://owned-archive.invalid/feed.xml".into(),
            },
        )
        .await
        .unwrap();
    let subscription = initial.subscription_id.to_string();
    let mut connection = hito.podcasts.store.pool.acquire().await.unwrap();
    let mut transaction = connection.begin().await.unwrap();
    sqlx::query("CREATE TEMP TABLE podcast_archive_fixture(user_id TEXT NOT NULL, subscription_id TEXT NOT NULL, library_id TEXT NOT NULL)")
        .execute(&mut *transaction).await.unwrap();
    let parameters = sqlx::query("INSERT INTO podcast_archive_fixture SELECT user_id,id,library_id FROM podcast_subscriptions WHERE id=? AND user_id=?")
        .bind(&subscription).bind(actor.user().id.as_str()).execute(&mut *transaction).await.unwrap();
    assert_eq!(parameters.rows_affected(), 1);
    sqlx::raw_sql(include_str!("archive_query_work.sql"))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let counts: (i64,i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM recordings),(SELECT count(*) FROM podcast_episodes),(SELECT count(*) FROM podcast_episodes WHERE published_at IS NULL),(SELECT count(*) FROM podcast_episodes WHERE published_at=1700000000),(SELECT count(*) FROM account_recording_credits)")
        .fetch_one(&mut *transaction).await.unwrap();
    assert_eq!(counts, (100_000, 100_000, 40_000, 3, 100_000));
    let foreign_key_errors: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
    assert!(foreign_key_errors.is_empty());
    transaction.commit().await.unwrap();
    connection.return_to_pool().await;
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "Owned archive author");
    assert_eq!(artists.items[0].recording_count, 100_000);

    let meter = WorkMeter::install(&hito.podcasts.store.pool).await;
    let exercise = AssertUnwindSafe(async {
        let mut measurements = Vec::new();
        let mut depth = 0u32;
        let mut after = None;
        for target in [0u32,59_998,99_980] {
            while depth < target {
                let step = u16::try_from((target-depth).min(200)).unwrap();
                let page = hito.podcasts.episodes(&actor, &subscription, PodcastEpisodeQuery {
                    limit: Some(step),
                    after,
                }).await.expect("actual public continuations must reach deep archive pages");
                assert_eq!(page.total,100_000);
                assert_eq!(page.items.len(),usize::from(step));
                after = Some(page.next_cursor.expect("archive traversal must have another page"));
                depth += u32::from(step);
            }
            let (page, work) = meter.measure(hito.podcasts.episodes(&actor, &subscription, PodcastEpisodeQuery {
                limit: Some(PAGE_SIZE),
                after,
            })).await;
            let page = page.expect("owned archive episodes must remain browsable");
            assert_eq!(page.total,100_000);
            assert_eq!(page.limit,PAGE_SIZE);
            assert_eq!(page.items.len(),usize::from(PAGE_SIZE));
            let expected_episodes = (target..target+u32::from(PAGE_SIZE)).map(expected_index)
                .map(|index| format!("a2000000-0000-4000-8000-{index:012x}"))
                .collect::<Vec<_>>();
            let actual_episodes = page.items.iter().map(|episode| episode.episode_id.to_string()).collect::<Vec<_>>();
            assert_eq!(actual_episodes,expected_episodes,"episode order at depth {target}");
            let expected_recordings = (target..target+u32::from(PAGE_SIZE)).map(expected_index)
                .map(|index| format!("a1000000-0000-4000-8000-{index:012x}"))
                .collect::<Vec<_>>();
            let actual_recordings = page.items.iter().map(|episode| episode.recording.id.clone()).collect::<Vec<_>>();
            assert_eq!(actual_recordings,expected_recordings,"recording order at depth {target}");
            assert_eq!(page.items[0].recording.title,format!("Archive episode {:06}",expected_index(target)));
            depth += u32::from(PAGE_SIZE);
            assert_eq!(page.next_cursor.is_some(),depth<100_000);
            after = page.next_cursor;
            eprintln!("Podcast archive public page VM_steps~ retained_episodes=100000 depth={target} limit={PAGE_SIZE} work={work}");
            measurements.push((target,work));
        }
        for (depth, work) in measurements {
            assert!(work>0);
            assert!(work<ARCHIVE_WORK_BUDGET,
                "Public podcast episode page at depth {depth} used approximately {work} VM steps with100,000 retained episodes; twenty-row budget={ARCHIVE_WORK_BUDGET}");
        }
    }).catch_unwind().await;
    meter.remove(&hito.podcasts.store.pool).await;
    hito.podcasts.store.pool.close().await;
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}

// The unreleased contract orders equal dates and the final undated group by descending episode ID.
fn expected_index(depth: u32) -> u32 {
    if depth < 60_000 {
        depth / 3 * 3 + 2 - depth % 3
    } else {
        159_999 - depth
    }
}
