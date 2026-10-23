#[path = "fixtures/podcast_service.rs"]
#[allow(dead_code)]
mod fixture;

use hito_core::{
    CoreConfig, Credentials, Error, HostDeviceId, PodcastNetworkConfig, SubscribePodcast, open,
};
use uuid::Uuid;

#[tokio::test]
async fn empty_userinfo_is_rejected_before_a_subscription_or_refresh_is_persisted() {
    let server = fixture::PodcastServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("podcast-admission").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-owner".into(),
            password: "owned podcast fixture password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let result = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: Uuid::new_v4(),
                feed_url: format!(
                    "http://@{}",
                    server.feed_url.strip_prefix("http://").unwrap()
                ),
            },
        )
        .await;

    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "empty userinfo must be rejected before URL normalization"
    );
    assert!(
        hito.podcasts
            .subscriptions(&actor)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "a rejected feed must leave no refresh work"
    );
    assert_eq!(server.feed_requests(), 0);
    assert_eq!(server.enclosure_requests(), 0);
}
