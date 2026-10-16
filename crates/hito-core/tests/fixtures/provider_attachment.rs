#[path = "playlist_service.rs"]
mod playlists;
pub use playlists::{ProviderServer, playlist};

use hito_core::{Actor, InstallPlugin, OfferPluginService, PluginInstallation, Services};
use serde_json::json;

pub async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &ProviderServer,
    provider: &str,
    capabilities: &[&str],
) -> PluginInstallation {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": provider, "name": "Attachment fixture",
                "protocol_version": 1, "capabilities": capabilities
            }),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Attachment fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}
