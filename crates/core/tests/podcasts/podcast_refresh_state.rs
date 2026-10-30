#[path = "../fixtures/podcast_conditional_redirect_service.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::PodcastConditionalRedirectServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, CoreConfig, Credentials, Error, HostDeviceId, PodcastEpisodeQuery,
    PodcastNetworkConfig, PodcastSubscription, RefreshPodcast, Services, SubscribePodcast, open,
};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn refresh_state_distinguishes_pending_failure_and_the_last_successful_archive() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-refresh-state.wav"), 79);
    let mut server = PodcastConditionalRedirectServer::start(wav).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(directory.path(), &server)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    assert!(
        joined,
        "the owned refresh-state service must shut down and join on every outcome"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public refresh-state workflow must finish within its deadline"),
    }
}

async fn exercise(directory: &Path, server: &PodcastConditionalRedirectServer) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-refresh-state").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "refresh-state-owner".into(),
            password: "owned podcast refresh-state password".into(),
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
    let initial = subscription_state(&hito, &actor, receipt.subscription_id).await;
    assert!(initial.refresh_pending);
    assert!(!initial.refresh_failed);
    assert_eq!(initial.last_refreshed_at, None);
    assert_eq!(initial.content_revision, 0);
    assert_public_subscription(&initial, &server.origin);
    let empty = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.items.is_empty());
    assert_eq!(empty.content_revision, 0);
    assert!(server.requests().await.is_empty());

    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let successful = subscription_state(&hito, &actor, receipt.subscription_id).await;
    assert!(!successful.refresh_pending);
    assert!(!successful.refresh_failed);
    assert!(successful.last_refreshed_at.is_some_and(|time| time > 0));
    assert_eq!(successful.content_revision, 1);
    assert_public_subscription(&successful, &server.origin);
    let success_time = successful.last_refreshed_at;
    let first = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(first.content_revision, 1);
    assert_eq!(first.total, 2);
    assert_eq!(first.items.len(), 2);
    assert_eq!(
        first.items[0].recording.inherited_title,
        fixture::FIRST_TITLE
    );
    assert_eq!(first.items[0].recording.artist, fixture::AUTHOR);
    let recording_id = Uuid::parse_str(&first.items[0].recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My retained refresh-state episode".into(),
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
    let retained = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(retained.content_revision, 1);
    assert_eq!(
        retained.items[0].recording.title,
        "My retained refresh-state episode"
    );
    assert!(retained.items[0].recording.liked);
    let retained_json = serde_json::to_value(&retained).unwrap();
    let mut page_fields = retained_json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    page_fields.sort_unstable();
    assert_eq!(
        page_fields,
        ["content_revision", "items", "limit", "next_cursor", "total"]
    );
    assert_eq!(server.requests().await.len(), 1);
    assert_eq!(server.enclosure_requests(), 0);

    server.redirect_to_unrequested_304();
    hito.podcasts
        .enqueue_refresh(
            &actor,
            &subscription,
            RefreshPodcast {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let queued = subscription_state(&hito, &actor, receipt.subscription_id).await;
    assert!(queued.refresh_pending);
    assert!(!queued.refresh_failed);
    assert_eq!(queued.last_refreshed_at, success_time);
    assert_eq!(queued.content_revision, 1);
    assert_eq!(server.requests().await.len(), 1);
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let failed = subscription_state(&hito, &actor, receipt.subscription_id).await;
    assert!(
        failed.refresh_pending && failed.refresh_failed,
        "a failed attempt queued for retry must expose pending and failure independently"
    );
    assert_eq!(failed.last_refreshed_at, success_time);
    assert_eq!(failed.content_revision, 1);
    assert_public_subscription(&failed, &server.origin);
    let after = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(after.content_revision, 1);
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        retained_json,
        "a failed refresh must retain the last published episode identities and account metadata"
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
    let history = hito.collection.activity(&actor).await.unwrap().history;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, recording_id.to_string());
    assert_eq!(history[0].speed, 1.5);
    for episode in &after.items {
        assert!(matches!(
            hito.downloads.status(&actor, &episode.recording.id).await,
            Err(Error::NotFound)
        ));
    }

    let other_credentials = || Credentials {
        username: "refresh-state-other".into(),
        password: "owned other refresh-state password".into(),
    };
    hito.identity
        .create_user(&actor, other_credentials())
        .await
        .unwrap();
    let other_session = hito.identity.login(other_credentials()).await.unwrap();
    let other = hito
        .identity
        .authenticate(&other_session.token)
        .await
        .unwrap();
    assert!(
        hito.podcasts
            .subscriptions(&other)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        hito.podcasts
            .episodes(&other, &subscription, PodcastEpisodeQuery::default())
            .await,
        Err(Error::NotFound)
    ));
    let requests = server.requests().await;
    assert_eq!(requests.len(), 3);
    for (request, expected_target) in requests.iter().zip([
        fixture::FEED_TARGET,
        fixture::FEED_TARGET,
        fixture::MOVED_TARGET,
    ]) {
        assert_eq!(request.method, axum::http::Method::GET);
        assert_eq!(request.target, expected_target);
        assert!(
            request
                .headers
                .get("accept")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("application/rss+xml"))
        );
    }
    assert_eq!(server.enclosure_requests(), 0);
    assert_eq!(server.unexpected_requests(), 0);
}

async fn subscription_state(hito: &Services, actor: &Actor, id: Uuid) -> PodcastSubscription {
    let mut subscriptions = hito.podcasts.subscriptions(actor).await.unwrap();
    assert_eq!(subscriptions.len(), 1);
    let subscription = subscriptions.remove(0);
    assert_eq!(subscription.subscription_id, id);
    subscription
}

fn assert_public_subscription(subscription: &PodcastSubscription, origin: &str) {
    let value = serde_json::to_value(subscription).unwrap();
    let mut fields = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "content_revision",
            "last_refreshed_at",
            "refresh_failed",
            "refresh_pending",
            "subscription_id",
            "title",
        ]
    );
    assert!(
        !serde_json::to_string(&value).unwrap().contains(origin),
        "subscription state must not disclose feed or enclosure addresses"
    );
}
