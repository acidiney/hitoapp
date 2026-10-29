use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    ArtistSimilarityPage, ArtistSimilarityRequest, CATALOG_SIMILARITY_VERSION, Capability,
    CatalogArtistRef, CatalogSimilarityCapabilities, CatalogSimilarityIdentity,
    CatalogSimilarityOperation, Manifest, PROTOCOL_VERSION, SimilarArtist,
};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinHandle};

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

struct State_ {
    declared: bool,
    descriptor_failure: Option<StatusCode>,
    reply: Reply,
    delay: Duration,
    observed: Vec<ObservedRequest>,
    unexpected: usize,
}

pub struct SimilarityServer {
    pub url: String,
    state: Arc<Mutex<State_>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl SimilarityServer {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(State_ {
            declared: true,
            descriptor_failure: None,
            reply: Reply::Similar(Vec::new()),
            delay: Duration::ZERO,
            observed: Vec::new(),
            unexpected: 0,
        }));
        let router = Router::new().fallback(respond).with_state(state.clone());
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
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn declare_similarity(&self, declared: bool) {
        self.state.lock().unwrap().declared = declared;
    }

    pub fn fail_descriptor(&self, status: Option<StatusCode>) {
        self.state.lock().unwrap().descriptor_failure = status;
    }

    pub fn reply(&self, reply: Reply) {
        self.state.lock().unwrap().reply = reply;
    }

    pub fn delay(&self, delay: Duration) {
        self.state.lock().unwrap().delay = delay;
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        self.state.lock().unwrap().observed.clone()
    }

    pub fn similarity_requests(&self) -> Vec<Value> {
        self.requests()
            .into_iter()
            .filter(|request| request.path == "/prefix/v1/catalog/similarity/artists")
            .map(|request| request.body)
            .collect()
    }

    pub fn unexpected(&self) -> usize {
        self.state.lock().unwrap().unexpected
    }

    pub async fn finish(&mut self) -> bool {
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

impl Drop for SimilarityServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn respond(State(state): State<Arc<Mutex<State_>>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, 4096).await else {
        return reject(&state);
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        let Ok(value) = serde_json::from_slice(&bytes) else {
            return reject(&state);
        };
        value
    };
    if parts.uri.query().is_some() {
        return reject(&state);
    }
    let path = parts.uri.path().to_owned();
    let (declared, reply, delay, descriptor_failure) = {
        let mut guard = state.lock().unwrap();
        if guard.observed.len() >= 32 {
            guard.unexpected += 1;
            return StatusCode::BAD_REQUEST.into_response();
        }
        guard.observed.push(ObservedRequest {
            method: parts.method.clone(),
            path: path.clone(),
            body: body.clone(),
        });
        (
            guard.declared,
            guard.reply.clone(),
            guard.delay,
            guard.descriptor_failure,
        )
    };
    match (parts.method.as_str(), path.as_str()) {
        ("GET", "/prefix/v1/manifest") => Json(Manifest {
            id: "org.hito.artist-similarity-fixture".into(),
            name: "Owned artist similarity".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .into_response(),
        ("GET", "/prefix/v1/catalog/similarity/capabilities") if descriptor_failure.is_some() => {
            descriptor_failure.unwrap().into_response()
        }
        ("GET", "/prefix/v1/catalog/similarity/capabilities") if declared => {
            Json(CatalogSimilarityCapabilities {
                similarity_version: CATALOG_SIMILARITY_VERSION,
                operations: vec![CatalogSimilarityOperation::ArtistSimilarity],
                artist_identity: CatalogSimilarityIdentity::MusicbrainzArtist,
            })
            .into_response()
        }
        ("GET", "/prefix/v1/catalog/similarity/capabilities") => {
            StatusCode::NOT_FOUND.into_response()
        }
        ("POST", "/prefix/v1/catalog/similarity/artists") if declared => {
            let Ok(request) = serde_json::from_value::<ArtistSimilarityRequest>(body) else {
                return reject(&state);
            };
            if request.validate().is_err() {
                return reject(&state);
            }
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            match reply {
                Reply::Similar(items) => Json(ArtistSimilarityPage {
                    artist_id: request.artist_id,
                    items: items
                        .into_iter()
                        .take(usize::from(request.limit))
                        .map(|(id, name, score)| SimilarArtist {
                            artist: CatalogArtistRef { id, name },
                            score,
                        })
                        .collect(),
                })
                .into_response(),
                Reply::Raw(value) => Json(value).into_response(),
                Reply::Status(status) => status.into_response(),
            }
        }
        _ => reject(&state),
    }
}

fn reject(state: &Mutex<State_>) -> Response {
    state.lock().unwrap().unexpected += 1;
    StatusCode::BAD_REQUEST.into_response()
}
