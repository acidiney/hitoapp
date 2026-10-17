use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, mpsc, oneshot};

pub const TOKEN: &str = "owned-recovery-provider-token";
pub const RECORDING: &str = "owned-recovery-recording";
pub const SOURCE: &str = "owned-recovery-exact-file";
pub const HANDLE: &str = "owned-recovery-accepted-handle";

pub struct Exchange {
    pub headers: HeaderMap,
    pub body: Value,
}

struct ProviderState {
    begin: mpsc::UnboundedSender<Exchange>,
    poll: mpsc::UnboundedSender<Exchange>,
    begins: AtomicUsize,
    polls: AtomicUsize,
    audio_requests: AtomicUsize,
    unexpected: AtomicUsize,
    ready: AtomicBool,
    audio: Vec<u8>,
}

pub struct Provider {
    pub url: String,
    state: Arc<ProviderState>,
    begin: Mutex<mpsc::UnboundedReceiver<Exchange>>,
    poll: Mutex<mpsc::UnboundedReceiver<Exchange>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Provider {
    pub async fn start(audio: Vec<u8>) -> Self {
        let (begin, begin_requests) = mpsc::unbounded_channel();
        let (poll, poll_requests) = mpsc::unbounded_channel();
        let state = Arc::new(ProviderState {
            begin,
            poll,
            begins: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
            audio_requests: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
            ready: AtomicBool::new(false),
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
            begin: Mutex::new(begin_requests),
            poll: Mutex::new(poll_requests),
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub async fn next_begin(&self) -> Exchange {
        self.begin.lock().await.recv().await.unwrap()
    }

    pub async fn next_poll(&self) -> Exchange {
        self.poll.lock().await.recv().await.unwrap()
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
        if !state.ready.load(Ordering::SeqCst)
            || request.uri().query() != Some("source_id=owned-recovery-exact-file")
        {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
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
            "id": "org.hito.preparation-recovery-fixture", "name": "Owned recovery provider",
            "protocol_version": 1, "capabilities": ["sources", "source_preparation"]
        }),
        "/v1/recordings/detail" if body["recording_id"] == RECORDING => json!({
            "id": RECORDING, "title": "Owned recording", "artist": "Owned artist"
        }),
        "/v1/sources" if body["recording_id"] == RECORDING => json!({
            "items": [{"id": SOURCE, "label": "Owned exact file", "ready": state.ready.load(Ordering::SeqCst)}]
        }),
        "/v1/sources/prepare"
            if body["recording_id"] == RECORDING && body["source_id"] == SOURCE =>
        {
            state.begins.fetch_add(1, Ordering::SeqCst);
            state
                .begin
                .send(Exchange {
                    headers,
                    body: body.clone(),
                })
                .unwrap();
            json!({
                "operation_id": body["operation_id"], "recording_id": RECORDING, "source_id": SOURCE,
                "submission": {
                    "outcome": "accepted", "handle": HANDLE,
                    "status": {"phase": "preparing", "progress_basis_points": 2500,
                        "eta_seconds": 15, "poll_after_seconds": 5}
                }
            })
        }
        "/v1/sources/preparation/status"
            if body["recording_id"] == RECORDING
                && body["source_id"] == SOURCE
                && body["handle"] == HANDLE =>
        {
            let previous = state.polls.fetch_add(1, Ordering::SeqCst);
            state
                .poll
                .send(Exchange {
                    headers,
                    body: body.clone(),
                })
                .unwrap();
            let phase = if previous == 0 {
                "submission_unknown"
            } else {
                state.ready.store(true, Ordering::SeqCst);
                "ready"
            };
            json!({
                "operation_id": body["operation_id"], "recording_id": RECORDING,
                "source_id": SOURCE, "handle": HANDLE, "status": {"phase": phase}
            })
        }
        _ => {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    Json(response).into_response()
}
