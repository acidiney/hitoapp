use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Query, Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

pub const TITLE: &str = "Owned matching track";
pub const ARTIST: &str = "Owned matching performer";

struct ProviderState {
    variant: &'static str,
    ready: AtomicBool,
    representation: AtomicUsize,
    offline: AtomicBool,
    requests: AtomicUsize,
    audio_requests: AtomicUsize,
    unexpected: AtomicUsize,
    audio: Vec<u8>,
}

pub struct Provider {
    pub url: String,
    state: Arc<ProviderState>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<bool>>,
}

impl Provider {
    pub async fn start(variant: &'static str, sample_offset: i16) -> Self {
        let state = Arc::new(ProviderState {
            variant,
            ready: AtomicBool::new(true),
            representation: AtomicUsize::new(0),
            offline: AtomicBool::new(false),
            requests: AtomicUsize::new(0),
            audio_requests: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
            audio: wav(sample_offset),
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

    pub fn recording_id(&self) -> String {
        self.state.recording_id()
    }
    pub fn source_id(&self) -> String {
        self.state.source_id()
    }
    pub fn audio(&self) -> &[u8] {
        &self.state.audio
    }
    pub fn requests(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }
    pub fn audio_requests(&self) -> usize {
        self.state.audio_requests.load(Ordering::SeqCst)
    }
    pub fn unexpected(&self) -> usize {
        self.state.unexpected.load(Ordering::SeqCst)
    }

    pub fn replace_unavailable(&self) {
        self.state.ready.store(false, Ordering::SeqCst);
        self.state.representation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn make_ready(&self) {
        self.state.ready.store(true, Ordering::SeqCst);
    }
    pub fn offline(&self) {
        self.state.offline.store(true, Ordering::SeqCst);
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

impl ProviderState {
    fn recording_id(&self) -> String {
        format!("owned-recording-{}", self.variant)
    }
    fn source_id(&self) -> String {
        format!(
            "owned-source-{}-{}",
            self.variant,
            self.representation.load(Ordering::SeqCst)
        )
    }
}

async fn exchange(State(state): State<Arc<ProviderState>>, request: Request) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    if !request
        .headers()
        .get("x-hito-plugin-protocol")
        .is_some_and(|value| value == "1")
    {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    if state.offline.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    if path == "/v1/audio" && (method == Method::GET || method == Method::HEAD) {
        if request
            .headers()
            .get("content-length")
            .is_some_and(|value| value != "0")
        {
            return invalid(&state);
        }
        let Ok(Query(query)) = Query::<HashMap<String, String>>::try_from_uri(request.uri()) else {
            return invalid(&state);
        };
        if query.len() != 1
            || query.get("source_id") != Some(&state.source_id())
            || !state.ready.load(Ordering::SeqCst)
        {
            return invalid(&state);
        }
        state.audio_requests.fetch_add(1, Ordering::SeqCst);
        return audio(
            &state.audio,
            &method,
            request
                .headers()
                .get("range")
                .and_then(|value| value.to_str().ok()),
        );
    }
    let Ok(bytes) = to_bytes(request.into_body(), 8192).await else {
        return invalid(&state);
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else if let Ok(body) = serde_json::from_slice(&bytes) {
        body
    } else {
        return invalid(&state);
    };
    let response = match (method, path.as_str()) {
        (Method::GET, "/v1/manifest") if body.is_null() => {
            json!({"id": format!("org.hito.manual-choice-{}", state.variant), "name": format!("Owned source {}", state.variant), "protocol_version": 1, "capabilities": ["sources"]})
        }
        (Method::POST, "/v1/recordings/detail")
            if body == json!({"recording_id": state.recording_id()}) =>
        {
            json!({"id": state.recording_id(), "title": TITLE, "artist": ARTIST, "duration_ms": 1000})
        }
        (Method::POST, "/v1/sources") if body == json!({"recording_id": state.recording_id()}) => {
            json!({"items": [{"id": state.source_id(), "label": format!("Owned source {}", state.variant), "ready": state.ready.load(Ordering::SeqCst)}]})
        }
        _ => return invalid(&state),
    };
    Json(response).into_response()
}

fn invalid(state: &ProviderState) -> Response {
    state.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::BAD_REQUEST.into_response()
}

fn audio(bytes: &[u8], method: &Method, range: Option<&str>) -> Response {
    let (status, first, last) = if let Some(range) = range {
        let Some((first, last)) = range
            .strip_prefix("bytes=")
            .and_then(|range| range.split_once('-'))
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let Ok(first) = first.parse::<usize>() else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let last = if last.is_empty() {
            bytes.len() - 1
        } else if let Ok(last) = last.parse::<usize>() {
            last.min(bytes.len() - 1)
        } else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if first > last || first >= bytes.len() {
            return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
        }
        (StatusCode::PARTIAL_CONTENT, first, last)
    } else {
        (StatusCode::OK, 0, bytes.len() - 1)
    };
    let mut response = Response::builder()
        .status(status)
        .header("content-type", "audio/wav")
        .header("accept-ranges", "bytes")
        .header("content-length", last - first + 1);
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            "content-range",
            format!("bytes {first}-{last}/{}", bytes.len()),
        );
    }
    response
        .body(if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(bytes[first..=last].to_vec())
        })
        .unwrap()
}

fn wav(offset: i16) -> Vec<u8> {
    let samples = 8000_u32;
    let mut bytes = Vec::with_capacity(44 + samples as usize * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for sample in 0..samples {
        bytes.extend_from_slice(&((((sample % 80) as i16 - 40) * 300) + offset).to_le_bytes());
    }
    bytes
}
