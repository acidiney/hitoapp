#[path = "fixtures/podcast_conditional_service.rs"]
mod fixture;
mod support;

use fixture::PodcastConditionalServer;
use futures_util::FutureExt;
use hito_core::{
    Action, AudioRequest, CoreConfig, Credentials, Error, HostDeviceId, OpenedAudio,
    PodcastEpisodeQuery, PodcastNetworkConfig, SubscribePodcast, open,
};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn a_due_conditional_check_preserves_the_published_archive_and_playback_authority() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let original = support::write_wav(&directory.path().join("owned-conditional.wav"), 317);
    let mut server = PodcastConditionalServer::start(original.clone()).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(directory.path(), &server, &original)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    assert!(
        joined,
        "the owned conditional RSS service must shut down and join on success or panic"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public conditional refresh exercise must finish within its deadline"),
    }
}

async fn exercise(directory: &Path, server: &PodcastConditionalServer, original: &[u8]) {
    let hito = open(config(directory, &server.origin)).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "conditional-owner".into(),
            password: "owned conditional podcast password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let request_id = Uuid::new_v4();
    let subscribe = || SubscribePodcast {
        request_id,
        feed_url: server.feed_url.clone(),
    };
    let receipt = hito.podcasts.subscribe(&actor, subscribe()).await.unwrap();
    let subscription = receipt.subscription_id.to_string();
    assert!(server.requests().await.is_empty());
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
    assert_eq!(first.recording.artist, fixture::AUTHOR);
    let old_cursor = first_page
        .next_cursor
        .expect("two episodes must provide a continuation cursor");
    let recording_id = Uuid::parse_str(&first.recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My private conditional episode".into(),
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
        "My private conditional episode"
    );
    assert!(before.items[0].recording.liked);
    let second_id = before.items[1].episode_id;
    let second_recording = before.items[1].recording.id.clone();
    let archive_before = serde_json::to_value(&before).unwrap();
    let mut sources_before = Vec::new();
    for episode in &before.items {
        let sources = hito
            .catalog
            .sources(&actor, &episode.recording.id)
            .await
            .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].provider, "podcast");
        sources_before.push(serde_json::to_value(sources).unwrap());
        assert!(matches!(
            hito.downloads.status(&actor, &episode.recording.id).await,
            Err(Error::NotFound)
        ));
    }
    let grant = hito
        .catalog
        .preferred_playback_source(&actor, &first.recording.id)
        .await
        .unwrap();
    let source_id = grant.source_id().to_owned();
    assert_eq!(sources_before[0][0]["id"], source_id);
    assert!(!grant.is_local());
    assert_eq!(
        hito.collection
            .saved_position(&actor, recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        700
    );
    let history = hito.collection.activity(&actor).await.unwrap();
    assert_eq!(history.history.len(), 1);
    assert_eq!(history.history[0].id, first.recording.id);
    assert_eq!(history.history[0].speed, 1.5);
    let history_before = serde_json::to_value(history).unwrap();
    let initial_requests = server.requests().await;
    assert_eq!(initial_requests.len(), 1);
    assert_request(
        &initial_requests[0],
        server,
        fixture::FEED_TARGET,
        false,
        false,
    );
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
    let receipts_before = receipt_counts(&external).await;
    assert_eq!(receipts_before, (1, 0));
    let aged = sqlx::query(
        "UPDATE podcast_subscriptions SET next_refresh_at=unixepoch()-1 WHERE id=? AND active=1",
    )
    .bind(&subscription)
    .execute(&external)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(aged, 1);
    server.require_conditional_check();
    drop(hito);

    let hito = open(config(directory, &server.origin)).await.unwrap();
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "the public worker must admit the due subscription after reopening without a new client request"
    );
    let requests = server.requests().await;
    assert_eq!(requests.len(), 2);
    assert_request(&requests[1], server, fixture::FEED_TARGET, true, false);
    let (last_refreshed, next_refresh, error, now): (Option<i64>, i64, Option<String>, i64) =
        sqlx::query_as("SELECT last_refreshed_at,next_refresh_at,error,unixepoch() FROM podcast_subscriptions WHERE id=?")
            .bind(&subscription).fetch_one(&external).await.unwrap();
    assert!(
        error.is_none(),
        "a valid conditional 304 must complete successfully rather than queue a failed attempt"
    );
    assert!(
        next_refresh > now,
        "a completed check must schedule the next hourly refresh"
    );
    assert_eq!(next_refresh - last_refreshed.unwrap(), 3600);
    assert_eq!(receipt_counts(&external).await, receipts_before);

    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let after = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(&after).unwrap(), archive_before);
    for (episode, sources) in after.items.iter().zip(&sources_before) {
        assert_eq!(
            serde_json::to_value(
                hito.catalog
                    .sources(&actor, &episode.recording.id)
                    .await
                    .unwrap()
            )
            .unwrap(),
            *sources
        );
        assert!(matches!(
            hito.downloads.status(&actor, &episode.recording.id).await,
            Err(Error::NotFound)
        ));
    }
    let continued = hito
        .podcasts
        .episodes(
            &actor,
            &subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: Some(old_cursor),
            },
        )
        .await
        .expect("304 must preserve the archive content revision and its old continuation cursor");
    assert_eq!(continued.total, 2);
    assert_eq!(continued.items.len(), 1);
    assert_eq!(continued.items[0].episode_id, second_id);
    assert_eq!(continued.items[0].recording.id, second_recording);
    assert!(continued.next_cursor.is_none());
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
        .expect("304 must preserve the previously captured enclosure authority");
    let selected = hito
        .catalog
        .preferred_playback_source(&actor, &first.recording.id)
        .await
        .unwrap();
    assert_eq!(selected.source_id(), source_id);
    let replay = hito.podcasts.subscribe(&actor, subscribe()).await.unwrap();
    assert_eq!(replay.request_id, request_id);
    assert_eq!(replay.subscription_id, receipt.subscription_id);
    assert_eq!(replay.job_id, receipt.job_id);
    assert_eq!(receipt_counts(&external).await, receipts_before);
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "a subscription that is no longer due must not create another refresh job"
    );
    assert_eq!(server.requests().await.len(), 2);
    assert_eq!(
        server.enclosure_requests(),
        0,
        "conditional metadata work must not retrieve audio"
    );

    let OpenedAudio::Content(mut audio) = grant
        .open(AudioRequest::default())
        .await
        .expect("explicit playback through the original grant must survive 304")
    else {
        panic!("full explicit playback must return content");
    };
    assert_eq!(audio.content_type, "audio/wav");
    assert_eq!(audio.length, Some(original.len() as u64));
    assert_eq!(audio.total_length, Some(original.len() as u64));
    let mut received = Vec::new();
    let body = audio
        .body
        .as_mut()
        .expect("explicit playback must return a body");
    while let Some(chunk) = body.chunk().await.unwrap() {
        assert!(chunk.len() <= original.len().saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert_eq!(
        received, original,
        "the preserved grant must return the exact owned WAV"
    );
    let requests = server.requests().await;
    assert_eq!(requests.len(), 3);
    assert_request(&requests[2], server, fixture::AUDIO_TARGET, false, true);
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
    assert_eq!(receipt_counts(&external).await, receipts_before);
    external.close().await;
}

fn config(directory: &Path, origin: &str) -> CoreConfig {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-conditional-refresh").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![origin.to_owned()]).unwrap();
    config
}

async fn receipt_counts(database: &SqlitePool) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM podcast_subscription_receipts),(SELECT count(*) FROM podcast_refresh_receipts)")
        .fetch_one(database).await.unwrap()
}

fn assert_request(
    request: &fixture::ObservedRequest,
    server: &PodcastConditionalServer,
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
