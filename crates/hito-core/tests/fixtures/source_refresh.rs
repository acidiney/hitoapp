#![allow(dead_code)]
#[path = "content_service.rs"]
mod provider;
use crate::support;

use hito_core::{
    Actor, AdoptProviderRecording, InstallPlugin, OfferPluginService, Recording, Services,
};
use serde_json::json;

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub hito: Services,
    pub actor: Actor,
    pub server: provider::ProviderServer,
    pub recording: Recording,
    pub installation: hito_core::PluginInstallation,
}

impl Fixture {
    pub async fn start(sources: serde_json::Value) -> Self {
        let server = provider::ProviderServer::start().await;
        let (directory, hito, actor, _, _) = support::library("source-refresh").await;
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Source fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let installed = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: Some("refresh-private-token".into()),
                },
            )
            .await
            .unwrap();
        server.reply("v1/recordings/detail", json!({
            "id":"saved/item", "title":"Accepted title", "artist":"Accepted artist", "duration_ms":1000,
            "release":{"id":"accepted/album", "title":"Accepted album", "artist":"Accepted artist"}, "track_number":1
        })).await;
        server.reply("v1/sources", sources).await;
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &installed.id,
                AdoptProviderRecording {
                    installation_revision: installed.revision,
                    provider_recording_id: "saved/item".into(),
                },
            )
            .await
            .unwrap();
        Self {
            directory,
            hito,
            actor,
            server,
            recording,
            installation: installed,
        }
    }
}
