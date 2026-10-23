use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode},
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

pub const TITLE: &str = "Owned controls recording";
pub const ARTIST: &str = "Owned controls performer";
pub const REVIEWED_LABEL: &str = "Owned reviewed B";
pub const AUTOMATIC_LABEL: &str = "Owned automatic C";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Primary,
    Reviewed,
    Automatic,
}

impl Role {
    fn recording(self) -> &'static str {
        match self {
            Self::Primary => "owned-controls-a-recording",
            Self::Reviewed => "owned-controls-b-recording",
            Self::Automatic => "owned-controls-c-recording",
        }
    }

    fn provider(self) -> &'static str {
        match self {
            Self::Primary => "org.hito.controls-primary",
            Self::Reviewed => "org.hito.controls-reviewed",
            Self::Automatic => "org.hito.controls-automatic",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Primary => "owned-controls-a-audio",
            Self::Reviewed => "owned-controls-b-audio",
            Self::Automatic => "owned-controls-c-audio",
        }
    }
}

struct ProviderState {
    role: Role,
    requests: AtomicUsize,
    searches: AtomicUsize,
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
    pub async fn start(role: Role) -> Self {
        let state = Arc::new(ProviderState {
            role,
            requests: AtomicUsize::new(0),
            searches: AtomicUsize::new(0),
            audio_requests: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
            audio: wav(),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|_| panic!("the owned provider must bind"));
        let url = format!(
            "http://{}/",
            listener
                .local_addr()
                .unwrap_or_else(|_| panic!("the owned provider address must be available"))
        );
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

    pub fn recording_id(&self) -> &'static str {
        self.state.role.recording()
    }
    pub fn audio(&self) -> &[u8] {
        &self.state.audio
    }
    pub fn requests(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }
    pub fn searches(&self) -> usize {
        self.state.searches.load(Ordering::SeqCst)
    }
    pub fn audio_requests(&self) -> usize {
        self.state.audio_requests.load(Ordering::SeqCst)
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

fn recording(role: Role) -> Value {
    json!({"id": role.recording(), "title": TITLE, "artist": ARTIST, "duration_ms": 1000})
}

async fn exchange(State(state): State<Arc<ProviderState>>, request: Request) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    if !request
        .headers()
        .get("x-hito-plugin-protocol")
        .is_some_and(|value| value == "1")
    {
        return invalid(&state);
    }
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    if request.headers().contains_key("range") {
        return invalid(&state);
    }
    let Ok(bytes) = to_bytes(request.into_body(), 8192).await else {
        return invalid(&state);
    };
    if path == "/v1/audio" {
        let expected_query = format!("source_id={}", state.role.source());
        if state.role != Role::Automatic
            || ![Method::GET, Method::HEAD].contains(&method)
            || query.as_deref() != Some(expected_query.as_str())
            || !bytes.is_empty()
        {
            return invalid(&state);
        }
        state.audio_requests.fetch_add(1, Ordering::SeqCst);
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "audio/wav")
            .header("content-length", state.audio.len())
            .body(if method == Method::HEAD {
                Body::empty()
            } else {
                Body::from(state.audio.clone())
            })
            .unwrap_or_else(|_| panic!("owned audio headers must be valid"));
    }
    if query.is_some() {
        return invalid(&state);
    }
    let body = if bytes.is_empty() {
        Value::Null
    } else if let Ok(body) = serde_json::from_slice(&bytes) {
        body
    } else {
        return invalid(&state);
    };
    let response = match (method, path.as_str()) {
        (Method::GET, "/v1/manifest") if body.is_null() => {
            let capabilities = if state.role == Role::Automatic {
                json!(["search", "sources"])
            } else {
                json!(["sources"])
            };
            json!({"id": state.role.provider(), "name": "Owned controls provider", "protocol_version": 1, "capabilities": capabilities})
        }
        (Method::POST, "/v1/recordings/detail")
            if body == json!({"recording_id": state.role.recording()}) =>
        {
            recording(state.role)
        }
        (Method::POST, "/v1/sources")
            if body == json!({"recording_id": state.role.recording()}) =>
        {
            match state.role {
                Role::Primary => json!({"items": []}),
                Role::Reviewed => {
                    json!({"items": [{"id": state.role.source(), "label": REVIEWED_LABEL, "ready": false}]})
                }
                Role::Automatic => {
                    json!({"items": [{"id": state.role.source(), "label": AUTOMATIC_LABEL, "ready": true}]})
                }
            }
        }
        (Method::POST, "/v1/search")
            if state.role == Role::Automatic
                && body
                    == json!({"query": format!("{ARTIST} {TITLE}"), "cursor": null, "limit": 20}) =>
        {
            state.searches.fetch_add(1, Ordering::SeqCst);
            json!({"items": [recording(state.role)], "next_cursor": null})
        }
        _ => return invalid(&state),
    };
    Json(response).into_response()
}

fn invalid(state: &ProviderState) -> Response {
    state.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::BAD_REQUEST.into_response()
}

fn wav() -> Vec<u8> {
    hito_testkit::audio::pcm16_wav(8000, 1, 8000, |index| ((index % 80) as i16 - 40) * 300)
}
