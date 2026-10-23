use hito_core::test_support::podcast_audio_service::PodcastAudioServer;
use hito_core::test_support::support;

use hito_core::{
    Actor, CoreConfig, Credentials, Error, HostDeviceId, PodcastNetworkConfig, Services,
    SubscribePodcast, open,
};
use uuid::Uuid;

struct Fixture {
    _directory: tempfile::TempDir,
    server: PodcastAudioServer,
    hito: Services,
    owner: Actor,
}

impl Fixture {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("music")).unwrap();
        let wav = support::write_wav(&directory.path().join("owned-read-model.wav"), 211);
        let server = PodcastAudioServer::start(wav).await;
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
        Self {
            _directory: directory,
            server,
            hito,
            owner,
        }
    }

    async fn subscribe(&self) -> Uuid {
        let receipt = self
            .hito
            .podcasts
            .subscribe(
                &self.owner,
                SubscribePodcast {
                    request_id: Uuid::new_v4(),
                    feed_url: self.server.feed_url.clone(),
                },
            )
            .await
            .unwrap();
        assert!(
            self.hito
                .podcasts
                .run_next_refresh_until(std::future::pending())
                .await
                .unwrap()
        );
        receipt.subscription_id
    }

    async fn other_account(&self) -> Actor {
        self.hito
            .identity
            .create_user(
                &self.owner,
                Credentials {
                    username: "read-model-guest".into(),
                    password: "owned guest read model password".into(),
                },
            )
            .await
            .unwrap();
        let session = self
            .hito
            .identity
            .login(Credentials {
                username: "read-model-guest".into(),
                password: "owned guest read model password".into(),
            })
            .await
            .unwrap();
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap()
    }

    async fn finish(mut self) {
        assert!(self.server.finish().await, "the owned feed must join");
    }
}

#[tokio::test]
async fn a_subscription_is_read_by_identity_for_its_owner_only() {
    let fixture = Fixture::start().await;
    let id = fixture.subscribe().await;
    let listed = fixture
        .hito
        .podcasts
        .subscriptions(&fixture.owner)
        .await
        .unwrap();
    let found = fixture
        .hito
        .podcasts
        .subscription(&fixture.owner, &id.to_string())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&found).unwrap(),
        serde_json::to_value(&listed[0]).unwrap()
    );

    let guest = fixture.other_account().await;
    for unreadable in [
        id.to_string(),
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
    fixture.finish().await;
}
