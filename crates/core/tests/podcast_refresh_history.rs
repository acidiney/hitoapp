#[path = "fixtures/podcast_conditional_service.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::PodcastConditionalServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, CoreConfig, Credentials, Error, HostDeviceId, MetadataHistoryQuery,
    PodcastEpisodeQuery, PodcastNetworkConfig, RefreshPodcast, Services, SubscribePodcast, open,
};
use serde_json::{Value, json};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{collections::BTreeSet, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

const AUTOMATIC_CHECKS: usize = 36;

#[tokio::test]
async fn automatic_refresh_history_retains_client_receipts_and_the_published_private_state() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let audio = support::write_wav(&directory.path().join("owned-refresh-history.wav"), 419);
    let mut server = PodcastConditionalServer::start(audio).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(directory.path(), &server)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    assert!(
        joined,
        "the owned feed service must join on success or panic"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the bounded automatic refresh history exercise must finish"),
    }
}

async fn exercise(directory: &Path, server: &PodcastConditionalServer) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-refresh-history").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "history-owner".into(),
            password: "owned podcast refresh history password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let subscribe_id = Uuid::new_v4();
    let subscribe = || SubscribePodcast {
        request_id: subscribe_id,
        feed_url: server.feed_url.clone(),
    };
    let subscribed = hito.podcasts.subscribe(&actor, subscribe()).await.unwrap();
    assert_eq!(subscribed.request_id, subscribe_id);
    let subscription = subscribed.subscription_id.to_string();
    let subscribed_before = serde_json::to_value(&subscribed).unwrap();
    assert!(server.requests().await.is_empty());
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(server.requests().await.len(), 1);
    let page = hito
        .podcasts
        .episodes(
            &actor,
            &subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].recording.inherited_title,
        fixture::FIRST_TITLE
    );
    assert_eq!(page.items[0].recording.artist, fixture::AUTHOR);
    let cursor = page
        .next_cursor
        .expect("the two-episode archive must have a continuation");
    let recording_id = Uuid::parse_str(&page.items[0].recording.id).unwrap();
    let playlist_id = Uuid::new_v4();
    let occurrences = [Uuid::new_v4(), Uuid::new_v4()];
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My private retained episode".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My private retained artist".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 700,
                speed: 1.5,
            },
            Action::PlaylistCreate {
                playlist_id,
                name: "My retained podcast selection".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: occurrences[0],
                recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: occurrences[1],
                recording_id,
                position: 1,
            },
        ],
    )
    .await;
    let (before, published_at) = snapshot(&hito, &actor, &subscription, playlist_id).await;
    assert_eq!(
        before["episodes"]["items"][0]["recording"]["title"],
        "My private retained episode"
    );
    assert_eq!(
        before["episodes"]["items"][0]["recording"]["artist"],
        "My private retained artist"
    );
    assert_eq!(before["episodes"]["items"][0]["recording"]["liked"], true);
    assert_eq!(before["recordings"][0]["position_ms"], 700);
    assert_eq!(before["activity"]["history"].as_array().unwrap().len(), 1);
    assert_eq!(before["activity"]["history"][0]["speed"], 1.5);
    assert_eq!(before["playlist"]["entries"].as_array().unwrap().len(), 2);
    for (index, occurrence) in occurrences.iter().enumerate() {
        assert_eq!(
            before["playlist"]["entries"][index]["entry_id"],
            occurrence.to_string()
        );
        assert_eq!(
            before["playlist"]["entries"][index]["recording_id"],
            recording_id.to_string()
        );
    }
    let grant = hito
        .playback
        .preferred_playback_source(&actor, &recording_id.to_string())
        .await
        .unwrap();
    assert!(!grant.is_local());
    assert_eq!(
        before["recordings"][0]["sources"][0]["id"],
        grant.source_id()
    );

    let database = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(directory.join("hito.db"))
                .foreign_keys(true),
        )
        .await
        .unwrap();
    wait_for_clock_after(&database, published_at).await;

    server.require_conditional_check();
    let refresh_id = Uuid::new_v4();
    let refreshed = hito
        .podcasts
        .enqueue_refresh(
            &actor,
            &subscription,
            RefreshPodcast {
                request_id: refresh_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(refreshed.request_id, refresh_id);
    assert_eq!(refreshed.subscription_id, subscribed.subscription_id);
    assert_ne!(refreshed.job_id, subscribed.job_id);
    let refreshed_before = serde_json::to_value(&refreshed).unwrap();
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(server.requests().await.len(), 2);
    let (after_manual, mut last_refreshed_at) =
        snapshot(&hito, &actor, &subscription, playlist_id).await;
    assert!(
        last_refreshed_at > published_at,
        "the successful manual 304 must advance the operational refresh timestamp"
    );
    assert_eq!(after_manual, before);

    let counts_before = client_counts(&database, &actor.user().id).await;
    assert_eq!(counts_before, (1, 1, 0, 2, 2));
    let mut automatic_jobs = Vec::new();
    for index in 0..AUTOMATIC_CHECKS {
        let aged = sqlx::query("UPDATE podcast_subscriptions SET next_refresh_at=unixepoch()-1 WHERE id=? AND active=1")
            .bind(&subscription).execute(&database).await.unwrap().rows_affected();
        assert_eq!(aged, 1);
        assert!(
            hito.podcasts
                .run_next_refresh_until(std::future::pending())
                .await
                .unwrap()
        );
        let newest: String = sqlx::query_scalar("SELECT id FROM podcast_refresh_jobs WHERE subscription_id=? AND status='complete' ORDER BY rowid DESC LIMIT 1")
            .bind(&subscription).fetch_one(&database).await.unwrap();
        assert!(!automatic_jobs.contains(&newest));
        assert_ne!(newest, subscribed.job_id.to_string());
        assert_ne!(newest, refreshed.job_id.to_string());
        automatic_jobs.push(newest);
        let jobs = retained_jobs(&database, &subscription).await;
        assert_eq!(jobs.len(), (index + 1).min(32) + 2);
        assert!(jobs.iter().all(|(_, status)| status == "complete"));
        assert_eq!(
            client_counts(&database, &actor.user().id).await,
            counts_before
        );
        assert_eq!(server.requests().await.len(), index + 3);
        assert_eq!(server.enclosure_requests(), 0);
        assert_eq!(server.unexpected_requests(), 0);
        let (subscriptions, refreshed_at) =
            subscription_snapshot(&hito, &actor, &subscription).await;
        assert!(
            refreshed_at >= last_refreshed_at,
            "successful automatic checks must not regress the refresh timestamp"
        );
        assert_eq!(subscriptions, before["subscriptions"]);
        last_refreshed_at = refreshed_at;
        grant
            .check()
            .await
            .expect("automatic 304 checks must preserve captured playback authority");
    }
    let retained = retained_jobs(&database, &subscription).await;
    let retained_ids: BTreeSet<String> = retained.iter().map(|(id, _)| id.clone()).collect();
    let expected: BTreeSet<String> = [subscribed.job_id.to_string(), refreshed.job_id.to_string()]
        .into_iter()
        .chain(automatic_jobs.iter().skip(AUTOMATIC_CHECKS - 32).cloned())
        .collect();
    assert_eq!(
        retained_ids, expected,
        "only the newest 32 automatic jobs and both client-referenced jobs may remain"
    );
    assert_eq!(retained.len(), 34);
    let (after_automatic, final_refreshed_at) =
        snapshot(&hito, &actor, &subscription, playlist_id).await;
    assert_eq!(after_automatic, before);
    assert_eq!(final_refreshed_at, last_refreshed_at);
    let continued = hito
        .podcasts
        .episodes(
            &actor,
            &subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: Some(cursor),
            },
        )
        .await
        .expect("pruning metadata-only history must preserve the original episode cursor");
    assert_eq!(continued.total, 2);
    assert_eq!(continued.items.len(), 1);
    assert_eq!(
        before["episodes"]["items"][1]["episode_id"],
        continued.items[0].episode_id.to_string()
    );
    assert_eq!(
        before["episodes"]["items"][1]["recording"]["id"],
        continued.items[0].recording.id
    );
    assert!(continued.next_cursor.is_none());
    let selected = hito
        .playback
        .preferred_playback_source(&actor, &recording_id.to_string())
        .await
        .unwrap();
    assert_eq!(selected.source_id(), grant.source_id());
    wait_for_clock_after(&database, final_refreshed_at).await;
    let requests_before_replay = server.requests().await.len();
    let replay = hito.podcasts.subscribe(&actor, subscribe()).await.unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), subscribed_before);
    let (subscriptions, replayed_at) = subscription_snapshot(&hito, &actor, &subscription).await;
    assert_eq!(subscriptions, before["subscriptions"]);
    assert_eq!(replayed_at, final_refreshed_at);
    assert_eq!(server.requests().await.len(), requests_before_replay);
    let replay = hito
        .podcasts
        .enqueue_refresh(
            &actor,
            &subscription,
            RefreshPodcast {
                request_id: refresh_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), refreshed_before);
    let (subscriptions, replayed_at) = subscription_snapshot(&hito, &actor, &subscription).await;
    assert_eq!(subscriptions, before["subscriptions"]);
    assert_eq!(replayed_at, final_refreshed_at);
    assert_eq!(server.requests().await.len(), requests_before_replay);
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(retained_jobs(&database, &subscription).await, retained);
    assert_eq!(
        client_counts(&database, &actor.user().id).await,
        counts_before
    );
    let (after_replay, replayed_at) = snapshot(&hito, &actor, &subscription, playlist_id).await;
    assert_eq!(after_replay, before);
    assert_eq!(replayed_at, final_refreshed_at);
    grant.check().await.unwrap();
    let requests = server.requests().await;
    assert_eq!(requests.len(), AUTOMATIC_CHECKS + 2);
    for (index, request) in requests.iter().enumerate() {
        assert_eq!(request.method, axum::http::Method::GET);
        assert_eq!(request.target, fixture::FEED_TARGET);
        if index == 0 {
            assert!(!request.headers.contains_key("if-none-match"));
            assert!(!request.headers.contains_key("if-modified-since"));
        } else {
            assert_eq!(request.headers["if-none-match"], fixture::ETAG);
            assert_eq!(request.headers["if-modified-since"], fixture::LAST_MODIFIED);
        }
    }
    assert_eq!(server.enclosure_requests(), 0);
    assert_eq!(server.unexpected_requests(), 0);
    database.close().await;
}

async fn snapshot(
    hito: &Services,
    actor: &Actor,
    subscription: &str,
    playlist: Uuid,
) -> (Value, i64) {
    let episodes = hito
        .podcasts
        .episodes(actor, subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    let mut recordings = Vec::new();
    for episode in &episodes.items {
        let id = &episode.recording.id;
        let sources = hito.catalog.sources(actor, id).await.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].provider, "podcast");
        assert!(sources[0].ready);
        assert!(matches!(
            hito.downloads.status(actor, id).await,
            Err(Error::NotFound)
        ));
        recordings.push(json!({
            "sources": sources,
            "metadata": hito.catalog.recording_metadata(actor, id, MetadataHistoryQuery::default()).await.unwrap(),
            "position_ms": hito.collection.saved_position(actor, Uuid::parse_str(id).unwrap()).await.unwrap().map(|saved| saved.position_ms)
        }));
    }
    let (subscriptions, last_refreshed_at) = subscription_snapshot(hito, actor, subscription).await;
    (
        json!({
            "subscriptions": subscriptions,
            "episodes": episodes,
            "recordings": recordings,
            "playlist": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
            "activity": hito.collection.activity(actor).await.unwrap(),
            "sync": hito.collection.events(actor, 0).await.unwrap()
        }),
        last_refreshed_at,
    )
}

async fn subscription_snapshot(hito: &Services, actor: &Actor, subscription: &str) -> (Value, i64) {
    let mut subscriptions =
        serde_json::to_value(hito.podcasts.subscriptions(actor).await.unwrap()).unwrap();
    let items = subscriptions.as_array_mut().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["subscription_id"], subscription);
    let last_refreshed_at = items[0]
        .as_object_mut()
        .unwrap()
        .remove("last_refreshed_at")
        .expect("the public subscription must expose its operational refresh field")
        .as_i64()
        .expect("a successfully published subscription must have a refresh timestamp");
    (subscriptions, last_refreshed_at)
}

async fn wait_for_clock_after(database: &SqlitePool, timestamp: i64) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
                .fetch_one(database)
                .await
                .unwrap();
            if now > timestamp {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("SQLite's observed clock must advance beyond the previous successful refresh");
}

async fn retained_jobs(database: &SqlitePool, subscription: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT id,status FROM podcast_refresh_jobs WHERE subscription_id=? ORDER BY rowid",
    )
    .bind(subscription)
    .fetch_all(database)
    .await
    .unwrap()
}

async fn client_counts(database: &SqlitePool, account: &str) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT
        (SELECT count(*) FROM podcast_subscription_receipts WHERE user_id=?),
        (SELECT count(*) FROM podcast_refresh_receipts WHERE user_id=?),
        (SELECT count(*) FROM podcast_unsubscribe_receipts WHERE user_id=?),
        (SELECT receipt_count FROM podcast_request_account_counts WHERE user_id=?),
        (SELECT receipt_count FROM podcast_request_total WHERE singleton=1)",
    )
    .bind(account)
    .bind(account)
    .bind(account)
    .bind(account)
    .fetch_one(database)
    .await
    .unwrap()
}
