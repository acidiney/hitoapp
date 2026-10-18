#![allow(dead_code)]
use crate::{fixture::ProviderServer, support};
use hito_core::{
    Actor, AdoptProviderRecording, InstallPlugin, OfferPluginService, PluginInstallation, Services,
    Source,
};

pub struct AudioLibrary {
    pub directory: tempfile::TempDir,
    pub music: std::path::PathBuf,
    pub server: ProviderServer,
    pub hito: Services,
    pub actor: Actor,
    pub source: Source,
    pub original: Vec<u8>,
    pub installation: PluginInstallation,
}
impl AudioLibrary {
    pub async fn start() -> Self {
        let server = ProviderServer::start().await;
        let (directory, hito, actor, _library, music) = support::library("provider-audio").await;
        let original = support::write_wav(&music.join("original.wav"), 10);
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: 1,
                    bearer_token: Some("private-audio-token".into()),
                },
            )
            .await
            .unwrap();
        server.reply("v1/recordings/detail", serde_json::json!({"id":"recording","title":"Fixture track","artist":"Fixture artist"})).await;
        server.reply("v1/sources", serde_json::json!({"items":[{"id":"audio:/1?private=value&other=bad","label":"Original","ready":true}]})).await;
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: 1,
                    provider_recording_id: "recording".into(),
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
        Self {
            directory,
            music,
            server,
            hito,
            actor,
            source,
            original,
            installation,
        }
    }
    pub async fn ready(&self) {
        self.server
            .audio(axum::http::StatusCode::OK, headers(), self.original.clone())
            .await;
    }
}
pub fn headers() -> axum::http::HeaderMap {
    axum::http::HeaderMap::from_iter([(
        "content-type".parse().unwrap(),
        "audio/wav".parse().unwrap(),
    )])
}
