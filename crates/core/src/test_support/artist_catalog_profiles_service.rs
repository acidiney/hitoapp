use axum::http::{HeaderMap, StatusCode};
use hito_plugin_sdk::{
    ArtistProfileRequest, CATALOG_VERSION, Capability, CatalogArtistKind, CatalogArtistProfile,
    CatalogArtistRef, CatalogCapabilities, CatalogOperation, MANIFEST_PATH, Manifest,
    PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin, Gate};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const CAPABILITIES: &str = "v1/catalog/capabilities";
const PROFILE: &str = "v1/catalog/artists/profile";
const PROFILE_LIMIT: usize = 16;

pub const ARTIST_ID: &str = "opaque:artist/one?credit=lead";
pub const MISSING_DESCRIPTOR_BODY: &str = "private-catalog-descriptor-404-body-canary";

#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub manifests: usize,
    pub descriptors: usize,
    pub profiles: usize,
    pub unexpected: usize,
}

pub struct CatalogServer {
    pub url: String,
    plugin: FakePlugin,
    descriptor_missing: Arc<AtomicBool>,
    profiles: Arc<Mutex<Vec<String>>>,
    held: Mutex<Option<Arc<Gate>>>,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let descriptor_missing = Arc::new(AtomicBool::new(false));
        let profiles = Arc::new(Mutex::new(Vec::<String>::new()));
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.artist-catalog-fixture".into(),
            name: "Owned artist catalog".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .get(CAPABILITIES, {
            let missing = descriptor_missing.clone();
            move |_| {
                if missing.load(Ordering::SeqCst) {
                    return fake::Reply::Raw {
                        status: StatusCode::NOT_FOUND,
                        headers: HeaderMap::new(),
                        body: MISSING_DESCRIPTOR_BODY.as_bytes().to_vec(),
                        chunked: false,
                    };
                }
                fake::Reply::Json(
                    serde_json::to_value(CatalogCapabilities {
                        catalog_version: CATALOG_VERSION,
                        operations: vec![CatalogOperation::ArtistProfile],
                    })
                    .unwrap(),
                )
            }
        })
        .post(PROFILE, {
            let profiles = profiles.clone();
            move |request| {
                let mut profiles = profiles.lock().unwrap();
                let parsed = serde_json::from_value::<ArtistProfileRequest>(request.json()).ok();
                let Some(parsed) = parsed.filter(|parsed| {
                    parsed.validate().is_ok()
                        && parsed.artist_id == ARTIST_ID
                        && profiles.len() < PROFILE_LIMIT
                }) else {
                    return fake::Reply::Refuse;
                };
                profiles.push(parsed.artist_id);
                fake::Reply::Json(serde_json::to_value(expected_profile()).unwrap())
            }
        })
        .require_json()
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
            descriptor_missing,
            profiles,
            held: Mutex::new(None),
        }
    }

    pub async fn counts(&self) -> Counts {
        Counts {
            manifests: self.plugin.requests_for(MANIFEST_PATH).len(),
            descriptors: self.plugin.requests_for(CAPABILITIES).len(),
            profiles: self.profiles.lock().unwrap().len(),
            unexpected: self.plugin.unexpected(),
        }
    }

    pub async fn profile_requests(&self) -> Vec<String> {
        self.profiles.lock().unwrap().clone()
    }

    pub fn set_descriptor_missing(&self, missing: bool) {
        self.descriptor_missing.store(missing, Ordering::SeqCst);
    }

    pub async fn hold_profile(&self) -> Arc<Gate> {
        let gate = self.plugin.hold(PROFILE);
        *self.held.lock().unwrap() = Some(gate.clone());
        gate
    }

    pub async fn release_profile(&self) {
        if let Some(gate) = self.held.lock().unwrap().take() {
            self.plugin.clear_fault(PROFILE);
            gate.release();
        }
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }
}

pub fn expected_profile() -> CatalogArtistProfile {
    CatalogArtistProfile {
        artist: CatalogArtistRef {
            id: ARTIST_ID.into(),
            name: "Verified provider artist".into(),
        },
        sort_name: Some("Provider artist, Verified".into()),
        kind: Some(CatalogArtistKind::Group),
        disambiguation: None,
        revision: Some("owned-profile-revision-1".into()),
    }
}
