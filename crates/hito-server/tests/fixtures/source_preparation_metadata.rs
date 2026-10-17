use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

pub const TOKEN: &str = "private-preparation-metadata-credential";
pub const RECORDING: &str = "private-preparation-metadata-recording";
pub const UNREADY: &str = "private-preparation-metadata-unready";
pub const READY: &str = "private-preparation-metadata-ready";
pub const ORIGINAL_LABEL: &str = "Archive FLAC";
pub const UPDATED_LABEL: &str = "Renamed archive FLAC";
pub const READY_LABEL: &str = "Ready preview";

struct ProviderState {
    preparation: bool,
    snapshot: AtomicUsize,
    requests: AtomicUsize,
    unexpected: AtomicUsize,
}

pub struct Provider {
    pub url: String,
    state: Arc<ProviderState>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<bool>>,
}

impl Provider {
    pub async fn start(preparation: bool) -> Self {
        let state = Arc::new(ProviderState {
            preparation,
            snapshot: AtomicUsize::new(0),
            requests: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let router = Router::new().fallback(exchange).with_state(state.clone());
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

    pub fn snapshot(&self, version: usize) {
        self.state.snapshot.store(version, Ordering::SeqCst);
    }

    pub fn requests(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }

    pub fn unexpected(&self) -> usize {
        self.state.unexpected.load(Ordering::SeqCst)
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

impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn exchange(State(state): State<Arc<ProviderState>>, request: Request) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    let expected_authorization = format!("Bearer {TOKEN}");
    let authorized = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(expected_authorization.as_str());
    let protocol = request
        .headers()
        .get("x-hito-plugin-protocol")
        .is_some_and(|value| value == "1");
    if !authorized || !protocol {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let path = request.uri().path().to_owned();
    let Ok(bytes) = to_bytes(request.into_body(), 32768).await else {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    };
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else if let Ok(body) = serde_json::from_slice(&bytes) {
        body
    } else {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    };
    let response = match path.as_str() {
        "/v1/manifest" => json!({
            "id": if state.preparation {
                "org.hito.preparation-metadata-fixture"
            } else {
                "org.hito.sources-only-metadata-fixture"
            },
            "name": "Owned source metadata provider",
            "protocol_version": 1,
            "capabilities": if state.preparation {
                vec!["sources", "source_preparation"]
            } else {
                vec!["sources"]
            }
        }),
        "/v1/recordings/detail" if body["recording_id"] == RECORDING => json!({
            "id": RECORDING, "title": "Owned source metadata recording", "artist": "Owned artist"
        }),
        "/v1/sources" if body["recording_id"] == RECORDING => {
            let version = state.snapshot.load(Ordering::SeqCst);
            let mut items = vec![json!({"id": READY, "label": READY_LABEL, "ready": true})];
            if version != 2 {
                items.push(json!({
                    "id": UNREADY,
                    "label": if version == 1 { UPDATED_LABEL } else { ORIGINAL_LABEL },
                    "ready": false
                }));
            }
            json!({"items": items})
        }
        _ => {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    Json(response).into_response()
}
