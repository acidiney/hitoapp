#![allow(dead_code)]

use super::{fixture::ProviderServer, support};
use hito_core::{
    Action, Actor, Credentials, Error, InstallPlugin, OfferPluginService, PluginService,
    PreviewPluginInstallation, Services,
};
use serde_json::{Value, json};
use std::{any::Any, future::pending, path::PathBuf, time::Duration};
use tokio::time::error::Elapsed;
use uuid::Uuid;

pub struct InstallationFixture {
    pub directory: tempfile::TempDir,
    pub hito: Services,
    pub actor: Actor,
    pub server: ProviderServer,
    pub service: PluginService,
    recording: String,
    playlist: String,
    preserved: Value,
    original_path: PathBuf,
    original: Vec<u8>,
}

impl InstallationFixture {
    pub async fn start() -> Self {
        let (directory, hito, actor, _library) = support::catalog().await;
        let original_path = directory.path().join("music/Alpha.wav");
        let original = std::fs::read(&original_path).unwrap();
        let recording = hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
            .remove(0);
        let recording_id = Uuid::parse_str(&recording.id).unwrap();
        let playlist_id = Uuid::new_v4();
        support::synchronize(
            &hito,
            &actor,
            vec![
                Action::RecordingTitleSet {
                    recording_id,
                    title: "private-installation-boundary-title".into(),
                },
                Action::LikeSet {
                    recording_id,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id,
                    position_ms: 700,
                    speed: 1.25,
                },
                Action::PlaylistCreate {
                    playlist_id,
                    name: "private-installation-boundary-playlist".into(),
                },
                Action::EntryInsert {
                    playlist_id,
                    entry_id: Uuid::new_v4(),
                    recording_id,
                    position: 0,
                },
                Action::EntryInsert {
                    playlist_id,
                    entry_id: Uuid::new_v4(),
                    recording_id,
                    position: 1,
                },
            ],
        )
        .await;
        let playlist = playlist_id.to_string();
        let preserved = private_state(&hito, &actor, &recording.id, &playlist).await;
        let server = ProviderServer::start().await;
        server
            .reply(
                "v1/manifest",
                json!({
                    "id": "org.hito.installation-boundary",
                    "name": "Owned installation boundary fixture",
                    "protocol_version": 1,
                    "capabilities": ["search", "sources"],
                    "content_policy": {"source_persistence": "explicit_download"}
                }),
            )
            .await;
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Owned installation boundary fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        Self {
            directory,
            hito,
            actor,
            server,
            service,
            recording: recording.id,
            playlist,
            preserved,
            original_path,
            original,
        }
    }

    pub fn preview_request(&self, token: Option<&str>) -> PreviewPluginInstallation {
        PreviewPluginInstallation {
            service_id: self.service.id.clone(),
            service_revision: self.service.revision,
            bearer_token: token.map(str::to_owned),
        }
    }

    pub fn install_request(&self, digest: &str, token: Option<&str>) -> InstallPlugin {
        InstallPlugin {
            service_id: self.service.id.clone(),
            service_revision: self.service.revision,
            bearer_token: token.map(str::to_owned),
            accept_manifest_digest: Some(digest.into()),
        }
    }

    pub async fn session(&self) -> (Actor, String) {
        let session = self
            .hito
            .identity
            .login(Credentials {
                username: "admin".into(),
                password: "catalog test password".into(),
            })
            .await
            .unwrap();
        let actor = self
            .hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        (actor, session.token)
    }

    pub async fn assert_preserved(&self) {
        assert!(
            private_state(&self.hito, &self.actor, &self.recording, &self.playlist).await
                == self.preserved,
            "connection workflows must preserve existing musical and personal state"
        );
        assert!(matches!(
            self.hito
                .downloads
                .status(&self.actor, &self.recording)
                .await,
            Err(Error::NotFound)
        ));
        assert!(
            !self
                .hito
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap()
        );
    }

    pub async fn assert_manifests_only(&self, count: usize) {
        assert_eq!(self.server.requests("v1/manifest").await.len(), count);
        for path in [
            "v1/search",
            "v1/recordings/detail",
            "v1/recordings/metadata",
            "v1/sources",
            "v1/playlists/detail",
            "v1/playlists/items",
            "v1/audio",
        ] {
            assert!(
                self.server.requests(path).await.is_empty(),
                "connection workflows must request only manifests"
            );
        }
    }

    pub async fn finish(self, outcome: Result<Result<(), Box<dyn Any + Send>>, Elapsed>) {
        let settled = tokio::time::timeout(
            Duration::from_secs(5),
            self.hito.downloads.finish_downloads(),
        )
        .await;
        let stopped = tokio::time::timeout(Duration::from_secs(2), self.server.shutdown()).await;
        assert!(matches!(settled, Ok(Ok(()))), "managed work must settle");
        assert!(stopped.is_ok(), "the owned provider must stop and join");
        assert!(
            std::fs::read(self.original_path).unwrap() == self.original,
            "connection workflows must preserve the owned original audio"
        );
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(_) => panic!("the installation boundary workflow exceeded its deadline"),
        }
    }
}

async fn private_state(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "playlist": hito.collection.playlist(actor, playlist).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap()
    })
}
