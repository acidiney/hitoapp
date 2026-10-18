#[path = "content_service.rs"]
mod content;
pub use content::ProviderServer;

use hito_core::{Actor, InstallPlugin, OfferPluginService, PluginInstallation, Services};
use serde_json::{Value, json};

#[allow(dead_code)]
pub async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &ProviderServer,
    capabilities: &[&str],
) -> PluginInstallation {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture", "name": "Playlist fixture",
                "protocol_version": 1, "capabilities": capabilities
            }),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}

pub async fn playlist(server: &ProviderServer, id: &str, snapshot: &str, items: Vec<Value>) {
    server
        .reply(
            "v1/playlists/detail",
            json!({
                "id": id, "name": "Fixture selection", "snapshot": snapshot,
                "total_count": items.len(), "attribution": {"label": "Playlist fixture"}
            }),
        )
        .await;
    server
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": id, "snapshot": snapshot, "items": items, "next_cursor": null
            }),
        )
        .await;
}
