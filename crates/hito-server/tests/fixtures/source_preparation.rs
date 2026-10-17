use axum::{
    Json, Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, mpsc, oneshot};

pub const RECORDING: &str = "private-provider-recording";
pub const SOURCE: &str = "private-exact-file-source";
pub const CONTROL_SOURCE: &str = "private-control-file-source";
pub const HANDLE: &str = "private-preparation-handle";

struct ProviderState {
    begin: mpsc::UnboundedSender<Exchange>,
    poll: mpsc::UnboundedSender<Exchange>,
    begins: AtomicUsize,
    polls: AtomicUsize,
    unexpected: AtomicUsize,
    prepared: AtomicBool,
    audio: Vec<u8>,
}

pub struct CapableProvider {
    pub url: String,
    state: Arc<ProviderState>,
    begin: Mutex<mpsc::UnboundedReceiver<Exchange>>,
    poll: Mutex<mpsc::UnboundedReceiver<Exchange>>,
    task: tokio::task::JoinHandle<()>,
}

pub struct Exchange {
    pub headers: HeaderMap,
    pub body: Value,
    reply: oneshot::Sender<Value>,
}

impl Exchange {
    pub fn accept(self) {
        let response = json!({
            "operation_id": self.body["operation_id"],
            "recording_id": self.body["recording_id"],
            "source_id": self.body["source_id"],
            "submission": {
                "outcome": "accepted",
                "handle": HANDLE,
                "status": {
                    "phase": "preparing", "progress_basis_points": 2500,
                    "eta_seconds": 15, "poll_after_seconds": 5
                }
            }
        });
        self.reply.send(response).unwrap();
    }

    pub fn ready(self) {
        let response = json!({
            "operation_id": self.body["operation_id"],
            "recording_id": self.body["recording_id"],
            "source_id": self.body["source_id"],
            "handle": self.body["handle"],
            "status": {"phase": "ready"}
        });
        self.reply.send(response).unwrap();
    }
}

impl CapableProvider {
    pub async fn start(audio: Vec<u8>) -> Self {
        let (begin, begin_requests) = mpsc::unbounded_channel();
        let (poll, poll_requests) = mpsc::unbounded_channel();
        let state = Arc::new(ProviderState {
            begin,
            poll,
            begins: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
            prepared: AtomicBool::new(false),
            audio,
        });
        let router = Router::new().fallback(exchange).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            state,
            begin: Mutex::new(begin_requests),
            poll: Mutex::new(poll_requests),
            task,
        }
    }

    pub async fn next_begin(&self) -> Exchange {
        self.begin.lock().await.recv().await.unwrap()
    }

    pub async fn next_poll(&self) -> Exchange {
        self.poll.lock().await.recv().await.unwrap()
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.state.begins.load(Ordering::SeqCst),
            self.state.polls.load(Ordering::SeqCst),
            self.state.unexpected.load(Ordering::SeqCst),
        )
    }
}

impl Drop for CapableProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn exchange(State(state): State<Arc<ProviderState>>, request: Request) -> Response {
    let expected_authorization = format!("Bearer {}", super::SERVICE_TOKEN);
    if request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(expected_authorization.as_str())
    {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let path = request.uri().path().to_owned();
    if path == "/v1/audio" {
        let expected = match request.uri().query() {
            Some("source_id=private-exact-file-source") => state.prepared.load(Ordering::SeqCst),
            Some("source_id=private-control-file-source") => true,
            _ => false,
        };
        if !expected {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
        return Response::builder()
            .header("content-type", "audio/wav")
            .header("content-length", state.audio.len())
            .body(Body::from(Bytes::copy_from_slice(&state.audio)))
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
            "id": "org.hito.capable-preparation-fixture", "name": "Owned preparation provider",
            "protocol_version": 1, "capabilities": ["sources", "source_preparation"]
        }),
        "/v1/recordings/detail" if body["recording_id"] == RECORDING => json!({
            "id": RECORDING, "title": "Owned exact recording", "artist": "Owned artist"
        }),
        "/v1/sources" if body["recording_id"] == RECORDING => json!({
            "items": [
                {"id": SOURCE, "label": "Owned exact file", "ready": state.prepared.load(Ordering::SeqCst)},
                {"id": CONTROL_SOURCE, "label": "Owned ready alternative", "ready": true}
            ]
        }),
        "/v1/sources/prepare" | "/v1/sources/preparation/status"
            if body["recording_id"] == RECORDING && body["source_id"] == SOURCE =>
        {
            let (reply, response) = oneshot::channel();
            let poll = path == "/v1/sources/preparation/status";
            let sender = if poll {
                state.polls.fetch_add(1, Ordering::SeqCst);
                &state.poll
            } else {
                state.begins.fetch_add(1, Ordering::SeqCst);
                &state.begin
            };
            if sender
                .send(Exchange {
                    headers,
                    body,
                    reply,
                })
                .is_err()
            {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            match response.await {
                Ok(response) => {
                    if poll && response["status"]["phase"] == "ready" {
                        state.prepared.store(true, Ordering::SeqCst);
                    }
                    response
                }
                Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
            }
        }
        _ => {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    Json(response).into_response()
}
