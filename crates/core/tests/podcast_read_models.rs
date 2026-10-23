#[path = "fixtures/podcast_service.rs"]
#[allow(dead_code)]
mod fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, Actor, CoreConfig, Credentials, Error, HostDeviceId, PodcastEpisodeQuery,
    PodcastNetworkConfig, RecordingProgress, Services, SubscribePodcast, open,
};
use uuid::Uuid;

struct Fixture {
    _directory: tempfile::TempDir,
    _server: fixture::PodcastServer,
    hito: Services,
    owner: Actor,
    subscription: String,
}

impl Fixture {
    async fn start() -> Self {
        let server = fixture::PodcastServer::start().await;
        server.publish_two_episodes("Owned read model author").await;
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("music")).unwrap();
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            directory.path().join("music"),
            HostDeviceId::new("podcast-read-models").unwrap(),
        );
        config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
        let hito = open(config).await.unwrap();
        let session = hito
            .identity
            .setup(Credentials {
                username: "read-model-owner".into(),
                password: "owned read model password".into(),
            })
            .await
            .unwrap();
        let owner = hito.identity.authenticate(&session.token).await.unwrap();
        let receipt = hito
            .podcasts
            .subscribe(
                &owner,
                SubscribePodcast {
                    request_id: Uuid::new_v4(),
                    feed_url: server.feed_url.clone(),
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
        Self {
            _directory: directory,
            _server: server,
            hito,
            owner,
            subscription: receipt.subscription_id.to_string(),
        }
    }

    async fn guest(&self) -> Actor {
        let credentials = || Credentials {
            username: "read-model-guest".into(),
            password: "owned guest read model password".into(),
        };
        self.hito
            .identity
            .create_user(&self.owner, credentials())
            .await
            .unwrap();
        let session = self.hito.identity.login(credentials()).await.unwrap();
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap()
    }

    async fn recording_ids(&self) -> Vec<Uuid> {
        self.hito
            .podcasts
            .episodes(
                &self.owner,
                &self.subscription,
                PodcastEpisodeQuery::default(),
            )
            .await
            .unwrap()
            .items
            .iter()
            .map(|episode| Uuid::parse_str(&episode.recording.id).unwrap())
            .collect()
    }
}

#[tokio::test]
async fn a_subscription_is_read_by_identity_for_its_owner_only() {
    let fixture = Fixture::start().await;
    let listed = fixture
        .hito
        .podcasts
        .subscriptions(&fixture.owner)
        .await
        .unwrap();
    let found = fixture
        .hito
        .podcasts
        .subscription(&fixture.owner, &fixture.subscription)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&found).unwrap(),
        serde_json::to_value(&listed[0]).unwrap()
    );

    let guest = fixture.guest().await;
    for unreadable in [
        fixture.subscription.clone(),
        Uuid::new_v4().to_string(),
        "not-a-uuid".into(),
    ] {
        assert!(matches!(
            fixture
                .hito
                .podcasts
                .subscription(&guest, &unreadable)
                .await,
            Err(Error::NotFound)
        ));
    }
}

#[tokio::test]
async fn episode_pages_carry_saved_progress_for_exactly_their_own_episodes() {
    let fixture = Fixture::start().await;
    let recordings = fixture.recording_ids().await;
    assert_eq!(recordings.len(), 2);
    support::synchronize(
        &fixture.hito,
        &fixture.owner,
        recordings
            .iter()
            .enumerate()
            .map(|(index, recording_id)| Action::ProgressSet {
                recording_id: *recording_id,
                position_ms: 1_000 * (index as i64 + 1),
                speed: 1.25,
            })
            .collect(),
    )
    .await;
    let podcasts = &fixture.hito.podcasts;

    let first = podcasts
        .episodes_with_progress(
            &fixture.owner,
            &fixture.subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(first.episodes.items.len(), 1);
    assert_eq!(
        first.progress,
        vec![progress(recordings[0], 1_000)],
        "a page reports progress for its own episode only"
    );

    let second = podcasts
        .episodes_with_progress(
            &fixture.owner,
            &fixture.subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: first.episodes.next_cursor,
            },
        )
        .await
        .unwrap();
    assert_eq!(second.progress, vec![progress(recordings[1], 2_000)]);

    let plain = podcasts
        .episodes(
            &fixture.owner,
            &fixture.subscription,
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    let joined = podcasts
        .episodes_with_progress(
            &fixture.owner,
            &fixture.subscription,
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&joined.episodes).unwrap(),
        serde_json::to_value(&plain).unwrap()
    );
    assert_eq!(joined.progress.len(), 2);
}

#[tokio::test]
async fn oversized_progress_pages_stay_within_the_episode_page_cap() {
    let fixture = Fixture::start().await;
    let page = fixture
        .hito
        .podcasts
        .episodes_with_progress(
            &fixture.owner,
            &fixture.subscription,
            PodcastEpisodeQuery {
                limit: Some(u16::MAX),
                after: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.episodes.limit, 200);
    assert!(page.progress.is_empty());
}

#[tokio::test]
async fn another_accounts_episode_pages_and_progress_read_as_not_found() {
    let fixture = Fixture::start().await;
    let recordings = fixture.recording_ids().await;
    support::synchronize(
        &fixture.hito,
        &fixture.owner,
        vec![Action::ProgressSet {
            recording_id: recordings[0],
            position_ms: 500,
            speed: 1.0,
        }],
    )
    .await;
    let guest = fixture.guest().await;
    for subscription in [fixture.subscription.as_str(), "not-a-uuid"] {
        assert!(matches!(
            fixture
                .hito
                .podcasts
                .episodes_with_progress(&guest, subscription, PodcastEpisodeQuery::default())
                .await,
            Err(Error::NotFound)
        ));
    }
}

fn progress(recording_id: Uuid, position_ms: u64) -> RecordingProgress {
    RecordingProgress {
        recording_id,
        position_ms,
        speed: 1.25,
    }
}
