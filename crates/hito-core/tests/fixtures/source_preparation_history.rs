use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub const RECORDING: &str = "owned:recording/first";
pub const OTHER_RECORDING: &str = "owned:recording/second";
pub const TOKEN: &str = "owned-history-provider-token";

#[derive(Default)]
struct Counts {
    begins: AtomicUsize,
    polls: AtomicUsize,
    audio: AtomicUsize,
    unexpected: AtomicUsize,
}

pub struct Provider {
    pub url: String,
    counts: Arc<Counts>,
    task: tokio::task::JoinHandle<()>,
}

impl Provider {
    pub async fn start() -> Self {
        let counts = Arc::new(Counts::default());
        let router = Router::new().fallback(exchange).with_state(counts.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, counts, task }
    }

    pub fn work_counts(&self) -> (usize, usize, usize, usize) {
        (
            self.counts.begins.load(Ordering::SeqCst),
            self.counts.polls.load(Ordering::SeqCst),
            self.counts.audio.load(Ordering::SeqCst),
            self.counts.unexpected.load(Ordering::SeqCst),
        )
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn exchange(State(counts): State<Arc<Counts>>, request: Request) -> Response {
    let authorization = format!("Bearer {TOKEN}");
    if request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(authorization.as_str())
    {
        counts.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let path = request.uri().path().to_owned();
    let forbidden_work = match path.as_str() {
        "/v1/sources/prepare" => Some(&counts.begins),
        "/v1/sources/preparation/status" => Some(&counts.polls),
        "/v1/audio" => Some(&counts.audio),
        _ => None,
    };
    if let Some(counter) = forbidden_work {
        counter.fetch_add(1, Ordering::SeqCst);
        return StatusCode::NOT_IMPLEMENTED.into_response();
    }
    if path == "/v1/manifest" {
        return Json(json!({
            "id": "org.hito.preparation-history-fixture",
            "name": "Owned preparation history provider",
            "protocol_version": 1,
            "capabilities": ["sources", "source_preparation"]
        }))
        .into_response();
    }
    let body = to_bytes(request.into_body(), 4096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let recording = body["recording_id"].as_str();
    let response = match (path.as_str(), recording) {
        ("/v1/recordings/detail", Some(id @ (RECORDING | OTHER_RECORDING))) => json!({
            "id": id, "title": "Owned recording", "artist": "Owned artist"
        }),
        ("/v1/sources", Some(recording @ (RECORDING | OTHER_RECORDING))) => json!({
            "items": [{
                "id": if recording == RECORDING {
                    "owned:immutable-exact-file/sha256-1"
                } else {
                    "owned:immutable-exact-file/sha256-2"
                },
                "label": "Owned unavailable exact file",
                "ready": false
            }]
        }),
        _ => {
            counts.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    Json(response).into_response()
}
