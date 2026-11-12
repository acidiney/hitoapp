use super::core::{
    Actor, BeginPlaybackLease, CoreConfig, Credentials, HostDeviceId, Services,
    test_support::{database_owner::DatabaseOwner, support},
};
use std::{path::PathBuf, sync::Arc, time::Duration};

pub struct Case {
    pub directory: Arc<tempfile::TempDir>,
    owner: DatabaseOwner,
    pub services: Services,
    pub actor: Actor,
    pub token: String,
    pub library: String,
    pub recording: String,
    pub source: String,
    pub original: Vec<u8>,
}

impl Case {
    pub async fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original = support::write_wav(&music.join("owned.wav"), 73);
        let owner = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("owned-lease-host").unwrap(),
        );
        owner.attach(&mut config);
        let services = super::core::open(config).await.unwrap();
        let login = services.identity.setup(credentials()).await.unwrap();
        let actor = services.identity.authenticate(&login.token).await.unwrap();
        let library = services
            .catalog
            .libraries(&actor)
            .await
            .unwrap()
            .remove(0)
            .id;
        support::scan(&services, &actor, &library).await;
        let recording = services
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
            .remove(0)
            .id;
        let source = services
            .catalog
            .sources(&actor, &recording)
            .await
            .unwrap()
            .remove(0)
            .id;
        Self {
            directory,
            owner,
            services,
            actor,
            token: login.token,
            library,
            recording,
            source,
            original,
        }
    }
    pub fn path(&self) -> PathBuf {
        self.directory.path().join("music/owned.wav")
    }
    pub fn request(&self) -> BeginPlaybackLease {
        BeginPlaybackLease {
            recording_id: self.recording.clone(),
            source_id: self.source.clone(),
        }
    }
    pub async fn close(self) {
        self.services.playback.close().await.unwrap();
        self.services.replicas.close().await.unwrap();
        self.services.catalog_replication.close().await.unwrap();
        drop(self.services);
        let result = self.owner.retire(Duration::from_secs(5)).await;
        assert!(
            result.resources_retired && !result.deadline_exceeded,
            "SQLite worker resources must retire before owned storage"
        );
        assert_eq!(self.owner.observe().active_resources(), 0);
    }
}
pub fn credentials() -> Credentials {
    Credentials {
        username: "admin".into(),
        password: "owned lease test password".into(),
    }
}

pub async fn content(opened: super::core::OpenedAudio) -> Vec<u8> {
    let super::core::OpenedAudio::Content(mut audio) = opened else {
        panic!("expected content")
    };
    let mut body = audio.body.take().expect("body");
    let mut result = Vec::new();
    while let Some(bytes) = body.chunk().await.unwrap() {
        result.extend_from_slice(&bytes);
    }
    result
}
