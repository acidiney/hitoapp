use crate::test_support::work_meter::WorkMeter;
use crate::{CoreConfig, Credentials, HostDeviceId, RefreshPodcast, SubscribePodcast, open};
use futures_util::FutureExt;
use sqlx::Connection;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

const FEED_URL: &str = "https://podcast-query-work.invalid/feed.xml";
const REQUEST_WORK_BUDGET: u64 = 50_000;

#[tokio::test]
async fn podcast_requests_and_replays_stay_bounded_after_large_terminal_history() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("podcast-request-work").unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-request-owner".into(),
            password: "owned podcast query work password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let initial_request = Uuid::new_v4();
    let initial = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: initial_request,
                feed_url: FEED_URL.into(),
            },
        )
        .await
        .unwrap();
    let older_job = Uuid::parse_str("f4000000-0000-4000-8000-000000000001").unwrap();
    let latest_job = Uuid::parse_str("94000000-0000-4000-8000-000000000002").unwrap();
    let mut connection = hito.podcasts.store.pool.acquire().await.unwrap();
    let mut transaction = connection.begin().await.unwrap();
    sqlx::query(
        "CREATE TEMP TABLE podcast_request_work_fixture (
            user_id TEXT NOT NULL, subscription_id TEXT NOT NULL,
            feed_url TEXT NOT NULL, request_digest TEXT NOT NULL,
            older_job_id TEXT NOT NULL, latest_job_id TEXT NOT NULL)",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let parameters = sqlx::query(
        "INSERT INTO podcast_request_work_fixture
         SELECT user_id,subscription_id,?,request_digest,?,?
         FROM podcast_subscription_receipts WHERE user_id=? AND request_id=?",
    )
    .bind(FEED_URL)
    .bind(older_job.to_string())
    .bind(latest_job.to_string())
    .bind(actor.user().id.as_str())
    .bind(initial_request.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(parameters.rows_affected(), 1);
    sqlx::raw_sql(include_str!("query_work.sql"))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let fixture_counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users),
            (SELECT count(*) FROM podcast_refresh_jobs),
            (SELECT count(*) FROM podcast_subscription_receipts),
            (SELECT count(*) FROM podcast_refresh_receipts)",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(fixture_counts, (17, 60_003, 30_002, 30_001));
    let foreign_key_errors: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
    assert!(foreign_key_errors.is_empty());
    transaction.commit().await.unwrap();
    connection.return_to_pool().await;

    let meter = WorkMeter::install(&hito.podcasts.store.pool).await;
    let exercise = AssertUnwindSafe(async {
        let subscribe_request = Uuid::new_v4();
        let subscribe = || SubscribePodcast {
            request_id: subscribe_request,
            feed_url: FEED_URL.into(),
        };
        let (subscribed, subscribe_work) = meter
            .measure(hito.podcasts.subscribe(&actor, subscribe()))
            .await;
        let (subscription_replay, subscription_replay_work) = meter
            .measure(hito.podcasts.subscribe(&actor, subscribe()))
            .await;
        let subscription = initial.subscription_id.to_string();
        let refresh_request = Uuid::new_v4();
        let refresh = || RefreshPodcast {
            request_id: refresh_request,
        };
        let (refreshed, refresh_work) = meter
            .measure(
                hito.podcasts
                    .enqueue_refresh(&actor, &subscription, refresh()),
            )
            .await;
        let (refresh_replay, refresh_replay_work) = meter
            .measure(
                hito.podcasts
                    .enqueue_refresh(&actor, &subscription, refresh()),
            )
            .await;
        eprintln!(
            "Podcast public request work VM_steps~ history_jobs=60003 \
             subscribe={subscribe_work} subscribe_replay={subscription_replay_work} \
             refresh={refresh_work} refresh_replay={refresh_replay_work}"
        );

        let subscribed = subscribed.expect("subscribing to the existing owned feed must succeed");
        assert_eq!(subscribed.request_id, subscribe_request);
        assert_eq!(subscribed.subscription_id, initial.subscription_id);
        assert_eq!(subscribed.job_id, latest_job);
        let subscription_replay =
            subscription_replay.expect("replaying the subscription must succeed");
        assert_eq!(subscription_replay.request_id, subscribe_request);
        assert_eq!(subscription_replay.subscription_id, initial.subscription_id);
        assert_eq!(subscription_replay.job_id, subscribed.job_id);

        let refreshed = refreshed.expect("requesting another owned feed refresh must succeed");
        assert_eq!(refreshed.request_id, refresh_request);
        assert_eq!(refreshed.subscription_id, initial.subscription_id);
        assert_ne!(refreshed.job_id, latest_job);
        assert_ne!(refreshed.job_id, initial.job_id);
        let refresh_replay = refresh_replay.expect("replaying the refresh must succeed");
        assert_eq!(refresh_replay.request_id, refresh_request);
        assert_eq!(refresh_replay.subscription_id, initial.subscription_id);
        assert_eq!(refresh_replay.job_id, refreshed.job_id);
        assert!(subscribe_work > 0 && refresh_work > 0);
        for (operation, work) in [
            ("subscribe", subscribe_work),
            ("subscribe replay", subscription_replay_work),
            ("refresh", refresh_work),
            ("refresh replay", refresh_replay_work),
        ] {
            assert!(
                work < REQUEST_WORK_BUDGET,
                "Public podcast {operation} used approximately {work} VM steps with \
                 60,003 terminal jobs; budget={REQUEST_WORK_BUDGET}"
            );
        }
    })
    .catch_unwind()
    .await;
    meter.remove(&hito.podcasts.store.pool).await;
    hito.podcasts.store.pool.close().await;
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
