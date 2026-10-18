use crate::{fixture::ProviderServer, support};
use axum::http::{HeaderMap, StatusCode};
use hito_core::{
    Action, Actor, AdoptProviderRecording, Credentials, Error, InstallPlugin, OfferPluginService,
    PluginCredential, PluginInstallation, PluginRefresh, PreviewPluginInstallation, RefreshPlugin,
    Services,
};
use hito_plugin_sdk::SourcePersistence;
use serde_json::{Value, json};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tokio::net::TcpSocket;
use uuid::Uuid;

pub struct Library {
    pub hito: Services,
    pub actor: Actor,
    pub other: Actor,
    pub server: Option<ProviderServer>,
    pub original: Vec<u8>,
    original_path: PathBuf,
    _directory: tempfile::TempDir,
}

pub struct Recording {
    pub installation: PluginInstallation,
    pub id: String,
    pub source: String,
    pub playlist: String,
}

impl Library {
    pub async fn start() -> Self {
        let (directory, hito, actor, _library, music) =
            support::library("provider-policy-refresh").await;
        let original_path = music.join("original.wav");
        let original = support::write_wav(&original_path, 427);
        let credentials = || Credentials {
            username: "policy-other".into(),
            password: "owned policy account password".into(),
        };
        hito.identity
            .create_user(&actor, credentials())
            .await
            .unwrap();
        let session = hito.identity.login(credentials()).await.unwrap();
        let other = hito.identity.authenticate(&session.token).await.unwrap();
        let server = ProviderServer::start().await;
        Self {
            hito,
            actor,
            other,
            server: Some(server),
            original,
            original_path,
            _directory: directory,
        }
    }

    pub async fn prepare(&self, persistence: Option<&str>) -> Recording {
        let server = self.server.as_ref().unwrap();
        let mut manifest = json!({
            "id": "org.hito.policy-refresh-fixture",
            "name": "Owned policy refresh fixture",
            "protocol_version": 1,
            "capabilities": ["sources"]
        });
        if let Some(persistence) = persistence {
            manifest["content_policy"] = json!({"source_persistence": persistence});
        }
        server.reply("v1/manifest", manifest).await;
        server
            .reply(
                "v1/recordings/detail",
                json!({
                    "id": "private-policy-refresh-recording",
                    "title": "private-policy-refresh-title",
                    "artist": "private-policy-refresh-artist",
                    "duration_ms": 1000
                }),
            )
            .await;
        server
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": "private-policy-refresh-source",
                    "label": "Owned audio",
                    "ready": true
                }]}),
            )
            .await;
        server
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([(
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                )]),
                self.original.clone(),
            )
            .await;
        let service = self
            .hito
            .plugins
            .offer_service(
                &self.actor,
                OfferPluginService {
                    name: "Owned policy refresh fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let accept_manifest_digest = if persistence.is_some() {
            Some(
                self.hito
                    .plugins
                    .preview_installation(
                        &self.actor,
                        PreviewPluginInstallation {
                            service_id: service.id.clone(),
                            service_revision: service.revision,
                            bearer_token: None,
                        },
                    )
                    .await
                    .unwrap()
                    .digest,
            )
        } else {
            None
        };
        let installation = self
            .hito
            .plugins
            .install(
                &self.actor,
                InstallPlugin {
                    accept_manifest_digest,
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let adopted = self
            .hito
            .catalog
            .adopt_provider(
                &self.actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: installation.revision,
                    provider_recording_id: "private-policy-refresh-recording".into(),
                },
            )
            .await
            .unwrap();
        let sources = self
            .hito
            .catalog
            .sources(&self.actor, &adopted.id)
            .await
            .unwrap();
        assert_eq!(sources.len(), 1);
        assert!(sources[0].ready);
        let recording_id = Uuid::parse_str(&adopted.id).unwrap();
        let playlist_id = Uuid::new_v4();
        support::synchronize(
            &self.hito,
            &self.actor,
            vec![
                Action::PlaylistCreate {
                    playlist_id,
                    name: "private-policy-refresh-playlist".into(),
                },
                Action::EntryInsert {
                    playlist_id,
                    entry_id: Uuid::new_v4(),
                    recording_id,
                    position: 0,
                },
                Action::RecordingTitleSet {
                    recording_id,
                    title: "private-policy-refresh-personal-title".into(),
                },
                Action::RecordingArtistSet {
                    recording_id,
                    artist: "private-policy-refresh-personal-artist".into(),
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
            ],
        )
        .await;
        Recording {
            installation,
            id: adopted.id,
            source: sources[0].id.clone(),
            playlist: playlist_id.to_string(),
        }
    }

    pub async fn approve_playback_only(&self, previous: &PluginInstallation) -> PluginInstallation {
        let mut manifest = serde_json::to_value(&previous.manifest).unwrap();
        manifest["content_policy"] = json!({"source_persistence": "playback_only"});
        self.server
            .as_ref()
            .unwrap()
            .reply("v1/manifest", manifest)
            .await;
        let review = self
            .hito
            .plugins
            .refresh(
                &self.actor,
                &previous.id,
                RefreshPlugin {
                    revision: previous.revision,
                    accept_manifest_digest: None,
                    credential: PluginCredential::Keep,
                },
            )
            .await
            .unwrap();
        let PluginRefresh::ReviewRequired { manifest, digest } = review else {
            panic!("tightening persistence must require manifest review");
        };
        assert!(
            manifest.content_policy.unwrap().source_persistence == SourcePersistence::PlaybackOnly
        );
        let unchanged = self.hito.plugins.settings(&self.actor).await.unwrap();
        assert_eq!(unchanged.installations.len(), 1);
        assert_eq!(unchanged.installations[0].revision, previous.revision);
        assert!(
            serde_json::to_value(&unchanged.installations[0].manifest).unwrap()
                == serde_json::to_value(&previous.manifest).unwrap(),
            "an unaccepted review must preserve the approved declaration"
        );
        let accepted = self
            .hito
            .plugins
            .refresh(
                &self.actor,
                &previous.id,
                RefreshPlugin {
                    revision: previous.revision,
                    accept_manifest_digest: Some(digest),
                    credential: PluginCredential::Keep,
                },
            )
            .await
            .unwrap();
        let PluginRefresh::Updated { installation } = accepted else {
            panic!("the reviewed declaration must update the same installation");
        };
        assert_eq!(installation.id, previous.id);
        assert_eq!(installation.service_id, previous.service_id);
        assert_eq!(installation.revision, previous.revision + 1);
        assert!(!installation.credential_set);
        assert!(
            installation
                .manifest
                .content_policy
                .as_ref()
                .unwrap()
                .source_persistence
                == SourcePersistence::PlaybackOnly
        );
        installation
    }

    pub async fn private_state(&self, recording: &Recording) -> Value {
        let metadata = self
            .hito
            .catalog
            .recording_metadata(&self.actor, &recording.id, Default::default())
            .await
            .unwrap();
        json!({
            "account": self.actor.user(),
            "libraries": self.hito.catalog.libraries(&self.actor).await.unwrap(),
            "catalog": self.hito.catalog.recordings(&self.actor, Default::default()).await.unwrap(),
            "sources": self.hito.catalog.sources(&self.actor, &recording.id).await.unwrap(),
            "playlist": self.hito.collection.playlist(&self.actor, &recording.playlist).await.unwrap(),
            "activity": self.hito.collection.activity(&self.actor).await.unwrap(),
            "events": self.hito.collection.events(&self.actor, 0).await.unwrap(),
            "metadata": {"title": metadata.title, "artist": metadata.artist}
        })
    }

    pub async fn assert_isolated(&self, recording: &Recording) {
        assert_eq!(
            self.hito
                .catalog
                .recordings(&self.other, Default::default())
                .await
                .unwrap()
                .total,
            0
        );
        assert!(
            self.hito
                .plugins
                .settings(&self.other)
                .await
                .unwrap()
                .installations
                .is_empty()
        );
        assert!(matches!(
            self.hito.catalog.sources(&self.other, &recording.id).await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            self.hito.downloads.status(&self.other, &recording.id).await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            self.hito
                .collection
                .playlist(&self.other, &recording.playlist)
                .await,
            Err(Error::NotFound)
        ));
    }

    pub async fn stop_provider(&mut self) {
        let server = self
            .server
            .take()
            .expect("the owned provider must be running");
        let endpoint = url::Url::parse(&server.url).unwrap();
        let address = SocketAddr::new(
            endpoint.host_str().unwrap().parse().unwrap(),
            endpoint.port().unwrap(),
        );
        let probe_socket =
            TcpSocket::new_v4().expect("the owned closure probe socket must initialize");
        probe_socket
            .bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("the owned closure probe must bind loopback");
        assert_ne!(
            probe_socket
                .local_addr()
                .expect("the owned closure probe must have an address")
                .port(),
            address.port(),
            "the owned closure probe must use a distinct source port"
        );
        tokio::time::timeout(Duration::from_secs(2), server.shutdown())
            .await
            .expect("the owned provider must shut down and join");
        let probe = tokio::time::timeout(Duration::from_secs(1), probe_socket.connect(address))
            .await
            .expect("the stopped owned endpoint must reject connections promptly");
        if let Ok(stream) = &probe {
            let local = stream.local_addr().ok();
            let peer = stream.peer_addr().ok();
            eprintln!(
                "provider_shutdown_probe target_port={} local_port={} peer_port={} local_known={} peer_known={} local_eq_peer={} local_eq_target={} peer_eq_target={}",
                address.port(),
                local.map(|address| address.port()).unwrap_or(0),
                peer.map(|address| address.port()).unwrap_or(0),
                local.is_some(),
                peer.is_some(),
                local.zip(peer).is_some_and(|(local, peer)| local == peer),
                local == Some(address),
                peer == Some(address),
            );
        }
        assert!(
            probe.is_err(),
            "the upstream listener must actually be closed"
        );
    }

    pub async fn finish(mut self) {
        let settled = tokio::time::timeout(
            Duration::from_secs(5),
            self.hito.downloads.finish_downloads(),
        )
        .await;
        if self.server.is_some() {
            self.stop_provider().await;
        }
        assert!(
            matches!(settled, Ok(Ok(()))),
            "managed filesystem work must settle"
        );
        assert!(
            std::fs::read(self.original_path).unwrap() == self.original,
            "policy refresh must preserve the user-owned original bytes"
        );
    }
}
