#[path = "fixtures/podcast_conditional_redirect_service.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::PodcastConditionalRedirectServer;
use futures_util::FutureExt;
use hito_core::{
    Action, AudioRequest, CoreConfig, Credentials, Error, HostDeviceId, OpenedAudio,
    PodcastEpisodeQuery, PodcastNetworkConfig, RefreshPodcast, SubscribePodcast, open,
};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct SuccessfulFeed {
    last_refreshed_at: Option<i64>,
    next_refresh_at: i64,
    fetch_url: String,
    validator_url: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
}

#[tokio::test]
async fn a_redirected_unrequested_304_cannot_replace_the_successful_feed_state() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let original = support::write_wav(&directory.path().join("owned-redirect.wav"), 419);
    let mut server = PodcastConditionalRedirectServer::start(original.clone()).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(directory.path(), &server, &original)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    assert!(
        joined,
        "the owned redirect service must shut down and join on success or panic"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the public conditional redirect exercise must finish within its deadline")
        }
    }
}

async fn exercise(directory: &Path, server: &PodcastConditionalRedirectServer, original: &[u8]) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-conditional-redirect").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "redirect-owner".into(),
            password: "owned conditional redirect password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let receipt = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: Uuid::new_v4(),
                feed_url: server.feed_url.clone(),
            },
        )
        .await
        .unwrap();
    let subscription = receipt.subscription_id.to_string();
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let first_page = hito
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
    assert_eq!(first_page.total, 2);
    assert_eq!(first_page.items.len(), 1);
    let first = &first_page.items[0];
    assert_eq!(first.recording.inherited_title, fixture::FIRST_TITLE);
    let cursor = first_page
        .next_cursor
        .expect("the owned archive must have a continuation");
    let recording_id = Uuid::parse_str(&first.recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My private redirect episode".into(),
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
        ],
    )
    .await;
    let before = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(before.total, 2);
    assert_eq!(before.items.len(), 2);
    assert_eq!(
        before.items[0].recording.title,
        "My private redirect episode"
    );
    assert!(before.items[0].recording.liked);
    let second_id = before.items[1].episode_id;
    let second_recording = before.items[1].recording.id.clone();
    let archive_before = serde_json::to_value(&before).unwrap();
    let sources_before = serde_json::to_value(
        hito.catalog
            .sources(&actor, &first.recording.id)
            .await
            .unwrap(),
    )
    .unwrap();
    let grant = hito
        .catalog
        .preferred_playback_source(&actor, &first.recording.id)
        .await
        .unwrap();
    let source_id = grant.source_id().to_owned();
    assert_eq!(sources_before[0]["id"], source_id);
    assert!(!grant.is_local());
    let history = hito.collection.activity(&actor).await.unwrap();
    assert_eq!(history.history.len(), 1);
    assert_eq!(history.history[0].id, first.recording.id);
    assert_eq!(history.history[0].position_ms, 700);
    assert_eq!(history.history[0].speed, 1.5);
    let history_before = serde_json::to_value(history).unwrap();
    let requests = server.requests().await;
    assert_eq!(requests.len(), 1);
    assert_request(&requests[0], server, fixture::FEED_TARGET, false, false);
    assert_eq!(server.enclosure_requests(), 0);

    let external = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(directory.join("hito.db"))
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let successful = successful_feed(&external, &subscription).await;
    assert_eq!(successful.fetch_url, server.feed_url);
    assert_eq!(
        successful.validator_url.as_deref(),
        Some(server.feed_url.as_str())
    );
    assert_eq!(successful.etag.as_deref(), Some(fixture::ETAG));
    assert_eq!(
        successful.last_modified.as_deref(),
        Some(fixture::LAST_MODIFIED)
    );
    let last_success = successful
        .last_refreshed_at
        .expect("the initial 200 must have successful freshness");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
                .fetch_one(&external)
                .await
                .unwrap();
            if now > last_success {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the rejected check must occur after the last successful timestamp");
    server.redirect_to_unrequested_304();
    let refresh = hito
        .podcasts
        .enqueue_refresh(
            &actor,
            &subscription,
            RefreshPodcast {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let requests = server.requests().await;
    assert_eq!(requests.len(), 3);
    assert_request(&requests[1], server, fixture::FEED_TARGET, true, false);
    assert_request(&requests[2], server, fixture::MOVED_TARGET, false, false);
    let (status, error, attempts): (String, Option<String>, i64) =
        sqlx::query_as("SELECT status,error,attempts FROM podcast_refresh_jobs WHERE id=?")
            .bind(refresh.job_id.to_string())
            .fetch_one(&external)
            .await
            .unwrap();
    assert_eq!(
        status, "queued",
        "an unrequested redirected 304 must not complete the refresh"
    );
    assert_eq!(error.as_deref(), Some("feed_unavailable"));
    assert_eq!(attempts, 1);
    assert_eq!(
        successful_feed(&external, &subscription).await,
        successful,
        "rejected 304 must not record freshness, commit the permanent redirect or replace validators"
    );

    let after = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(&after).unwrap(), archive_before);
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
        .expect("a rejected response must leave the existing archive cursor valid");
    assert_eq!(continued.total, 2);
    assert_eq!(continued.items.len(), 1);
    assert_eq!(continued.items[0].episode_id, second_id);
    assert_eq!(continued.items[0].recording.id, second_recording);
    assert!(continued.next_cursor.is_none());
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .sources(&actor, &first.recording.id)
                .await
                .unwrap()
        )
        .unwrap(),
        sources_before
    );
    assert_eq!(
        hito.collection
            .saved_position(&actor, recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        700
    );
    assert_eq!(
        serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap(),
        history_before
    );
    grant
        .check()
        .await
        .expect("rejecting the unrelated 304 must retain the original audio grant");
    assert_eq!(
        hito.catalog
            .preferred_playback_source(&actor, &first.recording.id)
            .await
            .unwrap()
            .source_id(),
        source_id
    );
    for episode in &after.items {
        assert!(matches!(
            hito.downloads.status(&actor, &episode.recording.id).await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(server.requests().await.len(), 3);
    assert_eq!(server.enclosure_requests(), 0);

    let OpenedAudio::Content(mut audio) = grant
        .open(AudioRequest::default())
        .await
        .expect("explicit playback must still open the last successful enclosure")
    else {
        panic!("explicit full playback must return content");
    };
    assert_eq!(audio.content_type, "audio/wav");
    assert_eq!(audio.length, Some(original.len() as u64));
    let body = audio
        .body
        .as_mut()
        .expect("the owned playback must return a readable body");
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        assert!(chunk.len() <= original.len().saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, original);
    let requests = server.requests().await;
    assert_eq!(requests.len(), 4);
    assert_request(&requests[3], server, fixture::AUDIO_TARGET, false, true);
    assert_eq!(server.enclosure_requests(), 1);
    assert_eq!(server.unexpected_requests(), 0);
    assert_eq!(
        serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap(),
        history_before
    );
    assert!(matches!(
        hito.downloads.status(&actor, &first.recording.id).await,
        Err(Error::NotFound)
    ));
    external.close().await;
}

async fn successful_feed(database: &SqlitePool, subscription: &str) -> SuccessfulFeed {
    sqlx::query_as("SELECT last_refreshed_at,next_refresh_at,fetch_url,validator_url,etag,last_modified FROM podcast_subscriptions WHERE id=?")
        .bind(subscription).fetch_one(database).await.unwrap()
}

fn assert_request(
    request: &fixture::ObservedRequest,
    server: &PodcastConditionalRedirectServer,
    target: &str,
    conditional: bool,
    audio: bool,
) {
    assert_eq!(request.method, axum::http::Method::GET);
    assert_eq!(request.target, target);
    let mut expected = vec![
        "accept",
        "accept-encoding",
        "connection",
        "host",
        "user-agent",
    ];
    if conditional {
        expected.extend(["if-modified-since", "if-none-match"]);
    }
    expected.sort_unstable();
    let mut actual: Vec<&str> = request.headers.keys().map(|name| name.as_str()).collect();
    actual.sort_unstable();
    assert_eq!(actual, expected);
    assert_eq!(request.headers.len(), expected.len());
    assert_eq!(
        request.headers["host"],
        server.origin.strip_prefix("http://").unwrap()
    );
    assert_eq!(request.headers["accept-encoding"], "identity");
    assert_eq!(request.headers["connection"], "close");
    assert_eq!(
        request.headers["user-agent"],
        concat!("Hito/", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        request.headers["accept"],
        if audio {
            "audio/*,application/ogg"
        } else {
            "application/rss+xml, application/atom+xml, application/xml, text/xml;q=0.9"
        }
    );
    if conditional {
        assert_eq!(request.headers["if-none-match"], fixture::ETAG);
        assert_eq!(request.headers["if-modified-since"], fixture::LAST_MODIFIED);
    }
}
