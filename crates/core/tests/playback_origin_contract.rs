use hito_core::test_support::playback::LocalPlayback;
use hito_core::test_support::podcast_audio_service::PodcastAudioServer;
use hito_core::test_support::provider_audio::AudioLibrary;
use hito_core::test_support::support;
use hito_core::{
    Actor, AudioRequest, ConnectFolder, CoreConfig, Credentials, DownloadRequest, DownloadState,
    Error, HostDeviceId, OpenedAudio, PlaybackSource, PodcastEpisodeQuery, PodcastNetworkConfig,
    RefreshPodcast, Services, SubscribePodcast, open,
};
use std::{future::pending, time::Duration};
use uuid::Uuid;

struct Subject<'a> {
    hito: &'a Services,
    actor: &'a Actor,
    recording: String,
    source: String,
}

impl Subject<'_> {
    async fn admit(&self) -> Result<PlaybackSource, Error> {
        self.hito
            .playback
            .playback_source(self.actor, &self.recording, &self.source)
            .await
    }

    async fn admitted(&self) -> PlaybackSource {
        let grant = self.admit().await.expect("the source must be admitted");
        grant.check().await.expect("a new grant must be valid");
        let opened = grant
            .open(AudioRequest::default())
            .await
            .expect("a valid grant must open its audio");
        assert!(matches!(opened, OpenedAudio::Content(_)));
        grant
    }

    async fn logout_does_not_revive_a_grant(&self, username: &str, password: &str) {
        let session = self
            .hito
            .identity
            .login(Credentials {
                username: username.into(),
                password: password.into(),
            })
            .await
            .unwrap();
        let actor = self
            .hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        let grant = self
            .hito
            .playback
            .playback_source(&actor, &self.recording, &self.source)
            .await
            .unwrap();
        self.hito.identity.logout(&session.token).await.unwrap();
        assert_dead(&grant, "logout").await;
        assert!(
            self.hito
                .playback
                .playback_source(&actor, &self.recording, &self.source)
                .await
                .is_err()
        );
    }
}

async fn assert_dead(grant: &PlaybackSource, cause: &str) {
    tokio::time::timeout(Duration::from_secs(3), grant.revoked())
        .await
        .unwrap_or_else(|_| panic!("a grant must notify its owner after {cause}"));
    assert!(grant.check().await.is_err());
    assert!(grant.open(AudioRequest::default()).await.is_err());
}

#[tokio::test]
async fn a_local_grant_dies_when_its_folder_generation_changes_and_never_revives() {
    let f = LocalPlayback::start().await;
    let subject = Subject {
        hito: &f.services,
        actor: &f.actor,
        recording: f.recording.clone(),
        source: f.source.clone(),
    };
    subject
        .logout_does_not_revive_a_grant("admin", "indexing test password")
        .await;
    let grant = subject.admitted().await;
    support::scan(&f.services, &f.actor, &f.library).await;
    grant
        .check()
        .await
        .expect("an unchanged rescan preserves the located state");
    let folder = f
        .services
        .storage
        .folders(&f.actor, &f.library)
        .await
        .unwrap()
        .remove(0);
    f.services
        .storage
        .remove_folder(&f.actor, &folder.id, folder.revision)
        .await
        .unwrap();
    f.services
        .storage
        .connect_folder(
            &f.actor,
            &f.library,
            ConnectFolder {
                directory: folder.directory.unwrap(),
                name: None,
            },
        )
        .await
        .unwrap();
    assert!(f.services.storage.run_next_job().await.unwrap());
    assert_dead(&grant, "a folder reconnect").await;
    subject.admitted().await;
}

#[tokio::test]
async fn a_managed_grant_dies_when_its_download_is_replaced_and_never_revives() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let recording = f.source.recording_id.clone();
    let download = |request| {
        let (hito, actor, recording) = (f.hito.clone(), f.actor.clone(), recording.clone());
        async move {
            let receipt = hito
                .downloads
                .create(
                    &actor,
                    &recording,
                    DownloadRequest {
                        request_id: request,
                    },
                )
                .await
                .unwrap();
            assert!(hito.downloads.run_next_job_until(pending()).await.unwrap());
            let status = hito.downloads.status(&actor, &recording).await.unwrap();
            assert!(matches!(status.status, DownloadState::Ready));
            (receipt.intent_id, status.source_id.unwrap())
        }
    };
    let (intent, source) = download(Uuid::new_v4()).await;
    let subject = Subject {
        hito: &f.hito,
        actor: &f.actor,
        recording: recording.clone(),
        source,
    };
    subject
        .logout_does_not_revive_a_grant("admin", "indexing test password")
        .await;
    let grant = subject.admitted().await;
    let owned = f.directory.path().join("hito.downloads");
    for entry in std::fs::read_dir(&owned).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "media")
        {
            std::fs::remove_file(path).unwrap();
        }
    }
    let unavailable = f.hito.downloads.status(&f.actor, &recording).await.unwrap();
    assert!(matches!(unavailable.status, DownloadState::Failed));
    assert_eq!(unavailable.intent_id, intent);
    assert_dead(&grant, "the managed file was lost").await;
    let (_, replacement) = download(Uuid::new_v4()).await;
    assert_dead(&grant, "a replacement download").await;
    let replaced = Subject {
        source: replacement,
        ..subject
    };
    replaced.admitted().await;
}

#[tokio::test]
async fn a_provider_grant_dies_when_its_credential_is_cleared_and_never_revives() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let subject = Subject {
        hito: &f.hito,
        actor: &f.actor,
        recording: f.source.recording_id.clone(),
        source: f.source.id.clone(),
    };
    subject
        .logout_does_not_revive_a_grant("admin", "indexing test password")
        .await;
    let grant = subject.admitted().await;
    f.hito
        .plugins
        .clear_credential(&f.actor, &f.installation.id, 1)
        .await
        .unwrap();
    assert_dead(&grant, "credential removal").await;
}

#[tokio::test]
async fn a_podcast_grant_dies_when_its_enclosure_is_replaced_and_never_revives() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let original = support::write_wav(&directory.path().join("old-owned.wav"), 211);
    let replacement = support::write_wav(&directory.path().join("new-owned.wav"), 977);
    let mut server = PodcastAudioServer::start(original).await;
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("origin-contract").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-owner".into(),
            password: "owned contract password".into(),
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
    assert!(
        hito.podcasts
            .run_next_refresh_until(pending())
            .await
            .unwrap()
    );
    let subscription = receipt.subscription_id.to_string();
    let episode = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let first = hito
        .playback
        .preferred_playback_source(&actor, &episode.recording.id)
        .await
        .unwrap();
    let subject = Subject {
        hito: &hito,
        actor: &actor,
        recording: episode.recording.id.clone(),
        source: first.source_id().to_owned(),
    };
    subject
        .logout_does_not_revive_a_grant("podcast-owner", "owned contract password")
        .await;
    let grant = subject.admitted().await;
    server.publish_enclosure(replacement).await;
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
    assert!(
        hito.podcasts
            .run_next_refresh_until(pending())
            .await
            .unwrap()
    );
    assert_dead(&grant, "enclosure replacement").await;
    subject.admitted().await;
    assert!(server.finish().await);
}
