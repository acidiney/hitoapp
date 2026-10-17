use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, mpsc, oneshot};

pub const TOKEN: &str = "owned-unacknowledged-service-token";
pub const RECORDING: &str = "owned-unacknowledged-recording";
pub const SOURCE: &str = "owned-unacknowledged-exact-file";
pub const HANDLE: &str = "owned-upstream-accepted-but-never-acknowledged-handle";

pub struct Exchange {
    pub path: String,
    pub headers: HeaderMap,
    pub body: Value,
}

struct ProviderState {
    exchanges: mpsc::UnboundedSender<Exchange>,
    accepted: Mutex<HashMap<String, &'static str>>,
    begins: AtomicUsize,
    polls: AtomicUsize,
    audio_requests: AtomicUsize,
    unexpected: AtomicUsize,
    audio: Vec<u8>,
}

pub struct Provider {
    pub url: String,
    state: Arc<ProviderState>,
    exchanges: Mutex<mpsc::UnboundedReceiver<Exchange>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Provider {
    pub async fn start(audio: Vec<u8>) -> Self {
        let (exchanges, requests) = mpsc::unbounded_channel();
        let state = Arc::new(ProviderState {
            exchanges,
            accepted: Mutex::new(HashMap::new()),
            begins: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
            audio_requests: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
            audio,
        });
        let router = Router::new().fallback(exchange).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            state,
            exchanges: Mutex::new(requests),
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub async fn next_exchange(&self) -> Exchange {
        self.exchanges.lock().await.recv().await.unwrap()
    }

    pub async fn accepted_handle(&self, operation: &str) -> Option<&'static str> {
        self.state.accepted.lock().await.get(operation).copied()
    }

    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.state.begins.load(Ordering::SeqCst),
            self.state.polls.load(Ordering::SeqCst),
            self.state.audio_requests.load(Ordering::SeqCst),
            self.state.unexpected.load(Ordering::SeqCst),
        )
    }

    pub async fn finish(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(4), &mut task).await {
            Ok(result) => result.is_ok(),
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
    let authorization = format!("Bearer {TOKEN}");
    if request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(authorization.as_str())
    {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let path = request.uri().path().to_owned();
    if path == "/v1/audio" {
        state.audio_requests.fetch_add(1, Ordering::SeqCst);
        return Response::builder()
            .header("content-type", "audio/wav")
            .header("content-length", state.audio.len())
            .body(Body::from(state.audio.clone()))
            .unwrap();
    }
    let headers = request.headers().clone();
    let bytes = to_bytes(request.into_body(), 32768).await.unwrap();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let response = match path.as_str() {
        "/v1/manifest" => json!({
            "id": "org.hito.unacknowledged-fixture", "name": "Owned uncertain provider",
            "protocol_version": 1, "capabilities": ["sources", "source_preparation"]
        }),
        "/v1/recordings/detail" if body["recording_id"] == RECORDING => json!({
            "id": RECORDING, "title": "Owned uncertain recording", "artist": "Owned artist"
        }),
        "/v1/sources" if body["recording_id"] == RECORDING => json!({
            "items": [{"id": SOURCE, "label": "Owned uncertain file", "ready": false}]
        }),
        "/v1/sources/prepare"
            if body["recording_id"] == RECORDING && body["source_id"] == SOURCE =>
        {
            state.begins.fetch_add(1, Ordering::SeqCst);
            state
                .accepted
                .lock()
                .await
                .insert(body["operation_id"].as_str().unwrap().to_owned(), HANDLE);
            let prefix = format!(
                "{{\"operation_id\":{},\"recording_id\":{},\"source_id\":{},\"submission\":{{\"outcome\":\"accepted\",\"status\":",
                body["operation_id"], body["recording_id"], body["source_id"]
            );
            let _ = state.exchanges.send(Exchange {
                path,
                headers,
                body,
            });
            return Response::builder()
                .header("content-type", "application/json")
                .header("content-length", prefix.len())
                .body(Body::from(prefix))
                .unwrap();
        }
        "/v1/sources/preparation/status" => {
            state.polls.fetch_add(1, Ordering::SeqCst);
            let _ = state.exchanges.send(Exchange {
                path,
                headers,
                body,
            });
            return StatusCode::CONFLICT.into_response();
        }
        _ => {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    Json(response).into_response()
}
