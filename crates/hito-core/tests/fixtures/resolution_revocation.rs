use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, Semaphore, watch},
    task::JoinHandle,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Search,
    DetailA,
    SourcesA,
    DetailB,
    SourcesB,
}

struct StateData {
    events: Mutex<Vec<Event>>,
    entered: Semaphore,
    release: Semaphore,
    authenticated: AtomicBool,
    closed: watch::Receiver<bool>,
}

pub struct Provider {
    base_url: String,
    state: Arc<StateData>,
    closed: watch::Sender<bool>,
    task: Option<JoinHandle<std::io::Result<()>>>,
}

impl Provider {
    pub async fn start() -> Self {
        let (closed, receiver) = watch::channel(false);
        let state = Arc::new(StateData {
            events: Mutex::new(Vec::new()),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            authenticated: AtomicBool::new(true),
            closed: receiver.clone(),
        });
        let router = Router::new()
            .route("/metadata/v1/manifest", get(|| async { Json(json!({
                "id": "org.hito.resolution-primary", "name": "Owned metadata", "protocol_version": 1,
                "capabilities": ["sources"]
            })) }))
            .route("/audio/v1/manifest", get(|| async { Json(json!({
                "id": "org.hito.resolution-audio", "name": "Owned audio", "protocol_version": 1,
                "capabilities": ["search", "sources"]
            })) }))
            .route("/metadata/v1/recordings/detail", post(exchange))
            .route("/metadata/v1/sources", post(exchange))
            .route("/audio/v1/search", post(exchange))
            .route("/audio/v1/recordings/detail", post(exchange))
            .route("/audio/v1/sources", post(exchange))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let mut receiver = receiver;
                    let _ = receiver.changed().await;
                })
                .await
        });
        Self {
            base_url,
            state,
            closed,
            task: Some(task),
        }
    }

    pub fn metadata_url(&self) -> String {
        format!("{}metadata/", self.base_url)
    }
    pub fn audio_url(&self) -> String {
        format!("{}audio/", self.base_url)
    }
    pub async fn events(&self) -> Vec<Event> {
        self.state.events.lock().await.clone()
    }
    pub fn authenticated(&self) -> bool {
        self.state.authenticated.load(Ordering::SeqCst)
    }
    pub async fn candidate_sources_entered(&self) {
        self.state.entered.acquire().await.unwrap().forget();
    }
    pub fn release_candidate_sources(&self) {
        self.state.release.add_permits(1);
    }

    pub async fn close(mut self) -> bool {
        let _ = self.closed.send(true);
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(Ok(Ok(()))) => true,
            Ok(_) => false,
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
        let _ = self.closed.send(true);
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn exchange(
    State(state): State<Arc<StateData>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let path = uri.path();
    if path.starts_with("/audio/")
        && headers
            .get("authorization")
            .and_then(|header| header.to_str().ok())
            != Some("Bearer owned-resolution-private-token")
    {
        state.authenticated.store(false, Ordering::SeqCst);
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if path == "/audio/v1/search" {
        state.events.lock().await.push(Event::Search);
        return Json(json!({"items": [detail("owned/candidate-a"), detail("owned/candidate-b")], "next_cursor": null})).into_response();
    }
    let Some(id) = body.get("recording_id").and_then(Value::as_str) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if path == "/metadata/v1/recordings/detail" && id == "owned/primary" {
        return Json(detail(id)).into_response();
    }
    if path == "/metadata/v1/sources" && id == "owned/primary" {
        return Json(json!({"items": []})).into_response();
    }
    let (detail_event, sources_event) = match id {
        "owned/candidate-a" => (Event::DetailA, Event::SourcesA),
        "owned/candidate-b" => (Event::DetailB, Event::SourcesB),
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    if path == "/audio/v1/recordings/detail" {
        state.events.lock().await.push(detail_event);
        return Json(detail(id)).into_response();
    }
    if path == "/audio/v1/sources" {
        state.events.lock().await.push(sources_event);
        if id == "owned/candidate-a" {
            state.entered.add_permits(1);
            let mut closed = state.closed.clone();
            tokio::select! {
                _ = closed.changed() => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
                permit = state.release.acquire() => if let Ok(permit) = permit { permit.forget(); },
            }
        }
        return Json(json!({"items": [{
            "id": format!("{id}/source"), "label": "Owned audio source", "ready": id == "owned/candidate-b"
        }]})).into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}

fn detail(id: &str) -> Value {
    json!({"id": id, "title": "Owned accepted title", "artist": "Owned accepted artist", "duration_ms": 1000})
}
