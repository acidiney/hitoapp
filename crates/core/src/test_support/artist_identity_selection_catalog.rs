use axum::http::Method;
use hito_plugin_sdk::{
    ArtistProfileRequest, ArtistSearchRequest, CATALOG_DISCOVERY_VERSION, CATALOG_VERSION,
    Capability, CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities,
    CatalogDiscoveryCapabilities, CatalogDiscoveryOperation, CatalogOperation, Manifest,
    PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin};
use serde_json::{Value, json};

pub const CREDIT: &str = "C4 Pedro";
pub const ALPHA: &str = "owned:artist/alpha";
pub const BETA: &str = "owned:artist/beta";

pub struct CatalogServer {
    pub url: String,
    plugin: FakePlugin,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.artist-identity-fixture".into(),
            name: "Owned artist identities".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .get("v1/catalog/discovery/capabilities", |_| {
            fake::Reply::Json(
                serde_json::to_value(CatalogDiscoveryCapabilities {
                    discovery_version: CATALOG_DISCOVERY_VERSION,
                    operations: vec![CatalogDiscoveryOperation::ArtistSearch],
                })
                .unwrap(),
            )
        })
        .get("v1/catalog/capabilities", |_| {
            fake::Reply::Json(
                serde_json::to_value(CatalogCapabilities {
                    catalog_version: CATALOG_VERSION,
                    operations: vec![CatalogOperation::ArtistProfile],
                })
                .unwrap(),
            )
        })
        .post("v1/catalog/artists/search", |request| {
            match serde_json::from_value::<ArtistSearchRequest>(request.json()) {
                Ok(parsed) if parsed.validate().is_ok() && parsed.query == CREDIT => {
                    fake::Reply::Json(json!({
                        "query": parsed.query, "snapshot": "owned-identities-v1",
                        "items": [
                            {"artist": {"id": ALPHA, "name": CREDIT}, "kind": null, "disambiguation": "Owned first performer"},
                            {"artist": {"id": BETA, "name": CREDIT}, "kind": null, "disambiguation": "Owned namesake"}
                        ], "total_count": 2, "next_cursor": null
                    }))
                }
                _ => fake::Reply::Refuse,
            }
        })
        .post("v1/catalog/artists/profile", |request| {
            match serde_json::from_value::<ArtistProfileRequest>(request.json()) {
                Ok(parsed)
                    if parsed.validate().is_ok()
                        && [ALPHA, BETA].contains(&parsed.artist_id.as_str()) =>
                {
                    fake::Reply::Json(
                        serde_json::to_value(CatalogArtistProfile {
                            artist: CatalogArtistRef {
                                id: parsed.artist_id,
                                name: CREDIT.into(),
                            },
                            sort_name: None,
                            kind: None,
                            disambiguation: None,
                            revision: Some("owned-profile-v1".into()),
                        })
                        .unwrap(),
                    )
                }
                _ => fake::Reply::Refuse,
            }
        })
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
        }
    }

    pub fn requests(&self) -> Vec<(Method, String, Value)> {
        self.plugin
            .requests()
            .into_iter()
            .map(|request| {
                let body = request.json();
                (request.method, format!("/{}", request.operation), body)
            })
            .collect()
    }

    pub async fn finish(&mut self) {
        assert_eq!(self.plugin.unexpected(), 0);
        assert!(self.plugin.finish().await);
    }
}
