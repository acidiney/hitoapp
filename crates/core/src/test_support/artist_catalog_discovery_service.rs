pub use super::observed::ObservedRequest;
use super::observed::observed;
use hito_plugin_sdk::{
    ArtistProfileRequest, ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest,
    CATALOG_DISCOVERY_VERSION, CATALOG_VERSION, Capability, CatalogArtistKind,
    CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities, CatalogDiscoveryCapabilities,
    CatalogDiscoveryOperation, CatalogOperation, Manifest, PROTOCOL_VERSION,
};
pub use hito_testkit::fake_plugin::Gate as SearchGate;
use hito_testkit::fake_plugin::{self as fake, FakePlugin};
use std::sync::{Arc, Mutex};

const DISCOVERY_CAPABILITIES: &str = "v1/catalog/discovery/capabilities";
const CAPABILITIES: &str = "v1/catalog/capabilities";
const PROFILE: &str = "v1/catalog/artists/profile";
const SEARCH: &str = "v1/catalog/artists/search";
const REQUEST_LIMIT: usize = 8;

pub const FIRST_ARTIST: &str = "opaque:artist/one?credit=lead";
pub const SECOND_ARTIST: &str = "opaque:artist/two~á";
pub const SNAPSHOT: &str = "owned-artist-search-snapshot:1";
pub const TOKEN: &str = "private-artist-discovery-service-token";

pub struct DiscoveryServer {
    pub url: String,
    plugin: FakePlugin,
    held: Mutex<Option<Arc<SearchGate>>>,
}

impl DiscoveryServer {
    pub async fn start() -> Self {
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.artist-discovery-fixture".into(),
            name: "Owned artist discovery".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .get(DISCOVERY_CAPABILITIES, |_| {
            fake::Reply::Json(
                serde_json::to_value(CatalogDiscoveryCapabilities {
                    discovery_version: CATALOG_DISCOVERY_VERSION,
                    operations: vec![CatalogDiscoveryOperation::ArtistSearch],
                })
                .unwrap(),
            )
        })
        .get(CAPABILITIES, |_| {
            fake::Reply::Json(
                serde_json::to_value(CatalogCapabilities {
                    catalog_version: CATALOG_VERSION,
                    operations: vec![CatalogOperation::ArtistProfile],
                })
                .unwrap(),
            )
        })
        .post(PROFILE, |request| {
            match serde_json::from_value::<ArtistProfileRequest>(request.json()) {
                Ok(parsed) if parsed.validate().is_ok() && parsed.artist_id == SECOND_ARTIST => {
                    fake::Reply::Json(serde_json::to_value(expected_profile()).unwrap())
                }
                _ => fake::Reply::Refuse,
            }
        })
        .post(SEARCH, |request| {
            match serde_json::from_value::<ArtistSearchRequest>(request.json()) {
                Ok(parsed)
                    if parsed.validate().is_ok()
                        && parsed.cursor.is_none()
                        && parsed.snapshot.is_none()
                        && parsed.limit == 2 =>
                {
                    fake::Reply::Json(serde_json::to_value(expected_search(parsed.query)).unwrap())
                }
                _ => fake::Reply::Refuse,
            }
        })
        .strict(REQUEST_LIMIT)
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
            held: Mutex::new(None),
        }
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        observed(&self.plugin, REQUEST_LIMIT)
    }

    pub fn unexpected(&self) -> usize {
        self.plugin.unexpected()
    }

    pub fn hold_search(&self) -> Arc<SearchGate> {
        let gate = self.plugin.stall_body(SEARCH, 1);
        *self.held.lock().unwrap() = Some(gate.clone());
        gate
    }

    pub fn release_search(&self) {
        if let Some(gate) = self.held.lock().unwrap().take() {
            self.plugin.clear_fault(SEARCH);
            gate.release();
        }
    }

    pub async fn finish(&mut self) -> bool {
        self.release_search();
        self.plugin.finish().await
    }
}

fn expected_search(query: String) -> ArtistSearchPage {
    ArtistSearchPage {
        query,
        snapshot: Some(SNAPSHOT.into()),
        items: vec![
            ArtistSearchCandidate {
                artist: CatalogArtistRef {
                    id: FIRST_ARTIST.into(),
                    name: "Owned catalog ensemble".into(),
                },
                kind: Some(CatalogArtistKind::Group),
                disambiguation: Some("Owned acoustic ensemble".into()),
                country: None,
                begin_year: None,
            },
            ArtistSearchCandidate {
                artist: CatalogArtistRef {
                    id: SECOND_ARTIST.into(),
                    name: "Owned catalog ensemble".into(),
                },
                kind: None,
                disambiguation: None,
                country: None,
                begin_year: None,
            },
        ],
        total_count: Some(2),
        next_cursor: None,
    }
}

pub fn expected_profile() -> CatalogArtistProfile {
    CatalogArtistProfile {
        artist: CatalogArtistRef {
            id: SECOND_ARTIST.into(),
            name: "Owned catalog ensemble".into(),
        },
        sort_name: Some("Ensemble, Owned catalog".into()),
        kind: Some(CatalogArtistKind::Group),
        disambiguation: Some("Second namesake, verified by the owned profile".into()),
        revision: Some("owned-candidate-profile:2".into()),
    }
}
