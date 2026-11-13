use axum::http::StatusCode;
use hito_plugin_sdk::{
    ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest, CATALOG_DISCOVERY_VERSION,
    CATALOG_VERSION, Capability, CatalogArtistRef, CatalogCapabilities,
    CatalogDiscoveryCapabilities, CatalogDiscoveryOperation, CatalogOperation, HEALTH_PATH,
    HealthReport, Manifest, PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{FakePlugin, RecordedRequest, Reply};
use serde_json::{Value, json};

pub const DESCRIPTOR: &str = "v1/catalog/discovery/capabilities";
pub const SEARCH: &str = "v1/catalog/artists/search";
pub const OWNER_TOKEN: &str = "owned-member-search-owner-token";
pub const PEER_TOKEN: &str = "owned-member-search-peer-token";

pub async fn start() -> FakePlugin {
    FakePlugin::builder(Manifest {
        id: "org.hito.member-search-credentials".into(),
        name: "Owned member search credentials".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Catalog].into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .get(HEALTH_PATH, |_| {
        Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
    })
    .get("v1/catalog/capabilities", |_| {
        Reply::Json(
            serde_json::to_value(CatalogCapabilities {
                catalog_version: CATALOG_VERSION,
                operations: vec![CatalogOperation::ArtistProfile],
            })
            .unwrap(),
        )
    })
    .get(DESCRIPTOR, |request| {
        assert!(owner_request(request));
        descriptor()
    })
    .post(SEARCH, |request| {
        assert!(owner_request(request));
        let parsed: ArtistSearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(parsed.validate().is_ok());
        assert_eq!(request.json(), query());
        Reply::Json(serde_json::to_value(page()).unwrap())
    })
    .strict(32)
    .start()
    .await
}

pub fn owner_request(request: &RecordedRequest) -> bool {
    request
        .headers
        .get("authorization")
        .is_some_and(|header| header == format!("Bearer {OWNER_TOKEN}").as_str())
}

pub fn descriptor() -> Reply {
    Reply::Json(
        serde_json::to_value(CatalogDiscoveryCapabilities {
            discovery_version: CATALOG_DISCOVERY_VERSION,
            operations: vec![CatalogDiscoveryOperation::ArtistSearch],
        })
        .unwrap(),
    )
}

pub fn unsupported() -> Reply {
    Reply::Json(json!({"discovery_version": CATALOG_DISCOVERY_VERSION, "operations": []}))
}

pub fn query() -> Value {
    json!({"query": "Cy", "cursor": null, "snapshot": null, "limit": 2})
}

pub fn page() -> ArtistSearchPage {
    ArtistSearchPage {
        query: "Cy".into(),
        snapshot: Some("owned-member-credentials:1".into()),
        items: ["owned:member/cy-1", "owned:member/cy-2"]
            .into_iter()
            .map(|id| ArtistSearchCandidate {
                artist: CatalogArtistRef {
                    id: id.into(),
                    name: "Cy".into(),
                },
                kind: None,
                disambiguation: None,
                country: None,
                begin_year: None,
            })
            .collect(),
        total_count: Some(2),
        next_cursor: None,
    }
}

pub fn reject(plugin: &FakePlugin, operation: &str) {
    plugin.on(operation, |request| {
        assert!(owner_request(request));
        Reply::Status(StatusCode::UNAUTHORIZED)
    });
}

pub fn valid_search(plugin: &FakePlugin) {
    plugin.on(SEARCH, |request| {
        assert!(owner_request(request));
        assert_eq!(request.json(), query());
        Reply::Json(serde_json::to_value(page()).unwrap())
    });
}

pub fn malformed_search(plugin: &FakePlugin) {
    plugin.on(SEARCH, |request| {
        assert!(owner_request(request));
        assert_eq!(request.json(), query());
        let mut invalid = page();
        invalid.items[1].artist.id = invalid.items[0].artist.id.clone();
        Reply::Json(serde_json::to_value(invalid).unwrap())
    });
}
