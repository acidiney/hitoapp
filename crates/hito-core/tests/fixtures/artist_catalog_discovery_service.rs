use axum::{
    Json, Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::Stream;
use hito_plugin_sdk::{
    ArtistProfileRequest, ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest,
    CATALOG_DISCOVERY_VERSION, CATALOG_VERSION, Capability, CatalogArtistKind,
    CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities, CatalogDiscoveryCapabilities,
    CatalogDiscoveryOperation, CatalogOperation, Manifest, PROTOCOL_VERSION,
};
use serde_json::Value;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, oneshot},
    task::JoinHandle,
};

pub const FIRST_ARTIST: &str = "opaque:artist/one?credit=lead";
pub const SECOND_ARTIST: &str = "opaque:artist/two~á";
pub const SNAPSHOT: &str = "owned-artist-search-snapshot:1";
pub const TOKEN: &str = "private-artist-discovery-service-token";

#[derive(Clone)]
pub struct ObservedRequest {
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Value,
}

#[derive(Default)]
struct Requests {
    observed: Mutex<Vec<ObservedRequest>>,
    unexpected: AtomicUsize,
    held_search: Mutex<Option<Arc<SearchGate>>>,
}

pub struct SearchGate {
    entered: Semaphore,
    release: Arc<Semaphore>,
    retired: Semaphore,
}

impl SearchGate {
    #[allow(dead_code)]
    pub async fn entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }

    #[allow(dead_code)]
    pub async fn retired(&self) {
        self.retired.acquire().await.unwrap().forget();
    }
}

struct HeldSearchBody {
    first: Option<Bytes>,
    remaining: Option<Bytes>,
    released: Pin<Box<dyn Future<Output = ()> + Send>>,
    gate: Arc<SearchGate>,
}

impl Stream for HeldSearchBody {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(first) = self.first.take() {
            self.gate.entered.add_permits(1);
            return Poll::Ready(Some(Ok(first)));
        }
        if self.remaining.is_none() {
            return Poll::Ready(None);
        }
        match self.released.as_mut().poll(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => Poll::Ready(self.remaining.take().map(Ok)),
        }
    }
}

impl Drop for HeldSearchBody {
    fn drop(&mut self) {
        self.gate.retired.add_permits(1);
    }
}

pub struct DiscoveryServer {
    pub url: String,
    requests: Arc<Requests>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl DiscoveryServer {
    pub async fn start() -> Self {
        let requests = Arc::new(Requests::default());
        let router = Router::new().fallback(respond).with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        Self {
            url,
            requests,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        self.requests.observed.lock().unwrap().clone()
    }

    pub fn unexpected(&self) -> usize {
        self.requests.unexpected.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub fn hold_search(&self) -> Arc<SearchGate> {
        let gate = Arc::new(SearchGate {
            entered: Semaphore::new(0),
            release: Arc::new(Semaphore::new(0)),
            retired: Semaphore::new(0),
        });
        *self.requests.held_search.lock().unwrap() = Some(gate.clone());
        gate
    }

    pub fn release_search(&self) {
        if let Some(gate) = self.requests.held_search.lock().unwrap().take() {
            gate.release.add_permits(1);
        }
    }

    pub async fn finish(&mut self) -> bool {
        self.release_search();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => matches!(result, Ok(true)),
            Err(_) => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}

impl Drop for DiscoveryServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn respond(State(requests): State<Arc<Requests>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, 4096).await else {
        return reject(&requests);
    };
    let value = if body.is_empty() {
        Value::Null
    } else {
        let Ok(value) = serde_json::from_slice(&body) else {
            return reject(&requests);
        };
        value
    };
    if parts.uri.query().is_some() {
        return reject(&requests);
    }
    let path = parts.uri.path().to_owned();
    let mut observed = requests.observed.lock().unwrap();
    if observed.len() >= 8 {
        return reject(&requests);
    }
    observed.push(ObservedRequest {
        method: parts.method.clone(),
        path: path.clone(),
        headers: parts.headers,
        body: value.clone(),
    });
    drop(observed);
    match (parts.method.as_str(), path.as_str()) {
        ("GET", "/prefix/v1/manifest") if body.is_empty() => Json(Manifest {
            id: "org.hito.artist-discovery-fixture".into(),
            name: "Owned artist discovery".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .into_response(),
        ("GET", "/prefix/v1/catalog/discovery/capabilities") if body.is_empty() => {
            Json(CatalogDiscoveryCapabilities {
                discovery_version: CATALOG_DISCOVERY_VERSION,
                operations: vec![CatalogDiscoveryOperation::ArtistSearch],
            })
            .into_response()
        }
        ("GET", "/prefix/v1/catalog/capabilities") if body.is_empty() => {
            Json(CatalogCapabilities {
                catalog_version: CATALOG_VERSION,
                operations: vec![CatalogOperation::ArtistProfile],
            })
            .into_response()
        }
        ("POST", "/prefix/v1/catalog/artists/profile") => {
            let Ok(request) = serde_json::from_value::<ArtistProfileRequest>(value) else {
                return reject(&requests);
            };
            if request.validate().is_err() || request.artist_id != SECOND_ARTIST {
                return reject(&requests);
            }
            Json(expected_profile()).into_response()
        }
        ("POST", "/prefix/v1/catalog/artists/search") => {
            let Ok(request) = serde_json::from_value::<ArtistSearchRequest>(value) else {
                return reject(&requests);
            };
            if request.validate().is_err()
                || request.cursor.is_some()
                || request.snapshot.is_some()
                || request.limit != 2
            {
                return reject(&requests);
            }
            let page = ArtistSearchPage {
                query: request.query,
                snapshot: Some(SNAPSHOT.into()),
                items: vec![
                    ArtistSearchCandidate {
                        artist: CatalogArtistRef {
                            id: FIRST_ARTIST.into(),
                            name: "Owned catalog ensemble".into(),
                        },
                        kind: Some(CatalogArtistKind::Group),
                        disambiguation: Some("Owned acoustic ensemble".into()),
                    },
                    ArtistSearchCandidate {
                        artist: CatalogArtistRef {
                            id: SECOND_ARTIST.into(),
                            name: "Owned catalog ensemble".into(),
                        },
                        kind: None,
                        disambiguation: None,
                    },
                ],
                total_count: Some(2),
                next_cursor: None,
            };
            let held = requests.held_search.lock().unwrap().clone();
            if let Some(gate) = held {
                let mut bytes = Bytes::from(serde_json::to_vec(&page).unwrap());
                let length = bytes.len();
                let first = bytes.split_to(1);
                let release = gate.release.clone();
                let body = HeldSearchBody {
                    first: Some(first),
                    remaining: Some(bytes),
                    gate,
                    released: Box::pin(async move {
                        if let Ok(permit) = release.acquire_owned().await {
                            permit.forget();
                        }
                    }),
                };
                Response::builder()
                    .header("content-type", "application/json")
                    .header("content-length", length)
                    .body(Body::from_stream(body))
                    .unwrap()
            } else {
                Json(page).into_response()
            }
        }
        _ => reject(&requests),
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

fn reject(requests: &Requests) -> Response {
    requests.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::BAD_REQUEST.into_response()
}
