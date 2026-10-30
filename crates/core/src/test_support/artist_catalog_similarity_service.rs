use axum::http::{Method, StatusCode};
use hito_plugin_sdk::{
    ArtistSimilarityPage, ArtistSimilarityRequest, CATALOG_SIMILARITY_VERSION, Capability,
    CatalogArtistRef, CatalogSimilarityCapabilities, CatalogSimilarityIdentity,
    CatalogSimilarityOperation, Manifest, PROTOCOL_VERSION, SimilarArtist,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin, Fault, RecordedRequest};
use serde_json::Value;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const CAPABILITIES: &str = "v1/catalog/similarity/capabilities";
const ARTISTS: &str = "v1/catalog/similarity/artists";
const REQUEST_LIMIT: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedRequest {
    pub method: Method,
    pub path: String,
    pub body: Value,
}

#[derive(Clone)]
pub enum Reply {
    Similar(Vec<(String, String, Option<f64>)>),
    Raw(Value),
    Status(StatusCode),
}

struct Behavior {
    declared: bool,
    reply: Reply,
}

pub struct SimilarityServer {
    pub url: String,
    plugin: FakePlugin,
    behavior: Arc<Mutex<Behavior>>,
    rejected: Arc<AtomicUsize>,
}

impl SimilarityServer {
    pub async fn start() -> Self {
        let behavior = Arc::new(Mutex::new(Behavior {
            declared: true,
            reply: Reply::Similar(Vec::new()),
        }));
        let rejected = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(AtomicUsize::new(0));
        let guard = {
            let (rejected, seen) = (rejected.clone(), seen.clone());
            move |request: &RecordedRequest| {
                let over_limit = seen.fetch_add(1, Ordering::SeqCst) >= REQUEST_LIMIT;
                let malformed = !request.body.is_empty()
                    && serde_json::from_slice::<Value>(&request.body).is_err();
                let rejects = over_limit || malformed || request.query.is_some();
                if rejects {
                    rejected.fetch_add(1, Ordering::SeqCst);
                }
                rejects
            }
        };
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.artist-similarity-fixture".into(),
            name: "Owned artist similarity".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .get(CAPABILITIES, {
            let (behavior, guard) = (behavior.clone(), guard.clone());
            move |request| {
                if guard(request) {
                    return fake::Reply::Status(StatusCode::BAD_REQUEST);
                }
                if behavior.lock().unwrap().declared {
                    fake::Reply::Json(
                        serde_json::to_value(CatalogSimilarityCapabilities {
                            similarity_version: CATALOG_SIMILARITY_VERSION,
                            operations: vec![CatalogSimilarityOperation::ArtistSimilarity],
                            artist_identity: CatalogSimilarityIdentity::MusicbrainzArtist,
                        })
                        .unwrap(),
                    )
                } else {
                    fake::Reply::Status(StatusCode::NOT_FOUND)
                }
            }
        })
        .post(ARTISTS, {
            let (behavior, rejected) = (behavior.clone(), rejected.clone());
            move |request| {
                if guard(request) {
                    return fake::Reply::Status(StatusCode::BAD_REQUEST);
                }
                let behavior = behavior.lock().unwrap();
                let parsed = serde_json::from_value::<ArtistSimilarityRequest>(request.json())
                    .ok()
                    .filter(|parsed| behavior.declared && parsed.validate().is_ok());
                let Some(parsed) = parsed else {
                    rejected.fetch_add(1, Ordering::SeqCst);
                    return fake::Reply::Status(StatusCode::BAD_REQUEST);
                };
                match &behavior.reply {
                    Reply::Similar(items) => fake::Reply::Json(
                        serde_json::to_value(ArtistSimilarityPage {
                            artist_id: parsed.artist_id,
                            items: items
                                .iter()
                                .take(usize::from(parsed.limit))
                                .map(|(id, name, score)| SimilarArtist {
                                    artist: CatalogArtistRef {
                                        id: id.clone(),
                                        name: name.clone(),
                                    },
                                    score: *score,
                                })
                                .collect(),
                        })
                        .unwrap(),
                    ),
                    Reply::Raw(value) => fake::Reply::Json(value.clone()),
                    Reply::Status(status) => fake::Reply::Status(*status),
                }
            }
        })
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
            behavior,
            rejected,
        }
    }

    pub fn declare_similarity(&self, declared: bool) {
        self.behavior.lock().unwrap().declared = declared;
    }

    pub fn fail_descriptor(&self, status: Option<StatusCode>) {
        match status {
            Some(status) => self.plugin.fault(CAPABILITIES, Fault::Status(status)),
            None => self.plugin.clear_fault(CAPABILITIES),
        }
    }

    pub fn reply(&self, reply: Reply) {
        self.behavior.lock().unwrap().reply = reply;
    }

    pub fn delay(&self, delay: Duration) {
        if delay.is_zero() {
            self.plugin.clear_fault(ARTISTS);
        } else {
            self.plugin.fault(ARTISTS, Fault::Delay(delay));
        }
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        self.plugin
            .requests()
            .into_iter()
            .take(REQUEST_LIMIT)
            .map(|request| ObservedRequest {
                path: format!("/prefix/{}", request.operation),
                body: request.json(),
                method: request.method,
            })
            .collect()
    }

    pub fn similarity_requests(&self) -> Vec<Value> {
        self.requests()
            .into_iter()
            .filter(|request| request.path == format!("/prefix/{ARTISTS}"))
            .map(|request| request.body)
            .collect()
    }

    pub fn unexpected(&self) -> usize {
        self.rejected.load(Ordering::SeqCst) + self.plugin.unmatched()
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }
}
