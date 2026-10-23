use super::{community_service as community, content_service as provider, support};
use crate::{
    Action, Actor, AdoptProviderRecording, CommunityConfig, CoreConfig, Credentials, HostDeviceId,
    InstallPlugin, OfferPluginService, PluginInstallation, PreviewPluginInstallation, Services,
    Source,
};
use serde_json::{Value, json};
use std::{io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};
use uuid::Uuid;

pub struct Account {
    pub actor: Actor,
    pub installation: PluginInstallation,
    pub source: Source,
    playlist: Uuid,
}

impl Account {
    pub async fn private_state(&self, hito: &Services) -> Value {
        json!({
            "preferences": hito.community.preferences(&self.actor).await.unwrap(),
            "catalog": hito.catalog.recordings(&self.actor, Default::default()).await.unwrap(),
            "metadata": hito.catalog.recording_metadata(&self.actor, &self.source.recording_id, Default::default()).await.unwrap(),
            "playlist": hito.collection.playlist(&self.actor, &self.playlist.to_string()).await.unwrap(),
            "activity": hito.collection.activity(&self.actor).await.unwrap(),
            "events": hito.collection.events(&self.actor, 0).await.unwrap(),
            "progress": hito.collection.saved_position(&self.actor, Uuid::parse_str(&self.source.recording_id).unwrap()).await.unwrap().map(|saved| saved.position_ms),
        })
    }
}

pub struct Library {
    pub directory: tempfile::TempDir,
    pub accounts: [Account; 2],
    pub original: Vec<u8>,
    pub original_path: PathBuf,
    pub token_path: PathBuf,
    _provider: provider::ProviderServer,
    music: PathBuf,
}

impl Library {
    pub async fn prepare(remote: &community::CommunityServer) -> (Self, Services) {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("private-recovery-originals");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("private-original.wav");
        let original = support::write_wav(&original_path, 13);
        let token_path = directory.path().join("private-admin-token");
        let mut token = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&token_path)
            .unwrap();
        token.write_all(community::ADMIN_TOKEN.as_bytes()).unwrap();
        drop(token);
        let hito = crate::open(configuration(&directory, &music, remote, &token_path))
            .await
            .unwrap();
        let session = hito
            .identity
            .setup(credentials("recovery-admin"))
            .await
            .unwrap();
        let admin = hito.identity.authenticate(&session.token).await.unwrap();
        hito.identity
            .create_user(&admin, credentials("recovery-listener"))
            .await
            .unwrap();
        let listener = hito
            .identity
            .login(credentials("recovery-listener"))
            .await
            .unwrap();
        let listener = hito.identity.authenticate(&listener.token).await.unwrap();
        let provider = provider::ProviderServer::start().await;
        provider.reply("v1/manifest", json!({
            "id": "org.hito.jamendo", "name": "Owned recovery provider", "protocol_version": 1,
            "capabilities": ["search", "sources"], "content_policy": {"source_persistence": "playback_only"}
        })).await;
        provider.reply("v1/recordings/detail", json!({
            "id": "jamendo:track:v1:42", "title": "private-recovery-title", "artist": "private-recovery-artist", "duration_ms": 90_000
        })).await;
        provider.reply("v1/sources", json!({"items": [{"id": "jamendo:audio:v1:42:mp32", "label": "Owned audio", "ready": true}]})).await;
        let service = hito
            .plugins
            .offer_service(
                &admin,
                OfferPluginService {
                    name: "Owned recovery provider".into(),
                    base_url: provider.url.clone(),
                },
            )
            .await
            .unwrap();
        let first = account(&hito, admin, &service.id, service.revision).await;
        let second = account(&hito, listener, &service.id, service.revision).await;
        assert_ne!(first.source.recording_id, second.source.recording_id);
        (
            Self {
                directory,
                accounts: [first, second],
                original,
                original_path,
                token_path,
                _provider: provider,
                music,
            },
            hito,
        )
    }

    pub async fn open(&self, remote: &community::CommunityServer) -> Services {
        crate::open(configuration(
            &self.directory,
            &self.music,
            remote,
            &self.token_path,
        ))
        .await
        .unwrap()
    }

    pub fn assert_originals(&self) {
        assert!(
            std::fs::read(&self.original_path).unwrap() == self.original,
            "Credential recovery must preserve original audio"
        );
        assert!(
            std::fs::read(&self.token_path).unwrap() == community::ADMIN_TOKEN.as_bytes(),
            "Credential recovery must preserve the owned administrator file"
        );
    }
}

async fn account(hito: &Services, actor: Actor, service: &str, revision: i64) -> Account {
    let preview = hito
        .plugins
        .preview_installation(
            &actor,
            PreviewPluginInstallation {
                service_id: service.into(),
                service_revision: revision,
                bearer_token: Some("private-recovery-provider-token".into()),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.into(),
                service_revision: revision,
                accept_manifest_digest: Some(preview.digest),
                bearer_token: Some("private-recovery-provider-token".into()),
            },
        )
        .await
        .unwrap();
    let recording = hito
        .catalog
        .adopt_provider(
            &actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: "jamendo:track:v1:42".into(),
            },
        )
        .await
        .unwrap();
    let source = hito
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0);
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    let playlist = Uuid::new_v4();
    support::synchronize(
        hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "private-recovery-playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "private-recovery-personal-title".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 1_300,
                speed: 1.25,
            },
        ],
    )
    .await;
    Account {
        actor,
        installation,
        source,
        playlist,
    }
}

fn configuration(
    directory: &tempfile::TempDir,
    music: &std::path::Path,
    remote: &community::CommunityServer,
    token: &std::path::Path,
) -> CoreConfig {
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music.to_owned(),
        HostDeviceId::new("owned-credential-recovery").unwrap(),
    );
    config.community = Some(CommunityConfig::new(remote.url.clone(), token.to_owned()).unwrap());
    config
}

pub fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "owned recovery account password".into(),
    }
}
