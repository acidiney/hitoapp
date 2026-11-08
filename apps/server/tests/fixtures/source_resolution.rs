use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::oneshot;

pub const TARGET: &str = "owned-metadata-recording";
pub const REMIX: &str = "owned-remix-recording";
pub const CHOSEN: &str = "owned-studio-recording";
pub const TITLE: &str = "Owned recording";
pub const ARTIST: &str = "Owned performer";

#[derive(Default)]
struct StateData {
    without_exact_match: bool,
    searches: AtomicUsize,
    details: AtomicUsize,
    unexpected: AtomicUsize,
}

pub struct Provider {
    pub url: String,
    state: Arc<StateData>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<bool>>,
}

impl Provider {
    pub async fn start() -> Self {
        Self::start_with(StateData::default()).await
    }

    pub async fn start_without_exact_match() -> Self {
        Self::start_with(StateData {
            without_exact_match: true,
            ..StateData::default()
        })
        .await
    }

    async fn start_with(data: StateData) -> Self {
        let state = Arc::new(data);
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

    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.state.searches.load(Ordering::SeqCst),
            self.state.details.load(Ordering::SeqCst),
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
        match tokio::time::timeout(std::time::Duration::from_secs(3), &mut task).await {
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

fn recording(id: &str) -> Value {
    let mut item = json!({"id": id, "title": TITLE, "artist": ARTIST, "duration_ms": 12000});
    match id {
        "owned-live-recording" => {
            item["release"] =
                json!({"id": "owned-live-release", "title": "Live at Owned Hall", "artist": ARTIST})
        }
        "owned-other-performer" => item["artist"] = json!("Another performer"),
        REMIX => item["title"] = json!("Owned recording (Remix)"),
        "owned-long-recording" => item["duration_ms"] = json!(90000),
        _ => {}
    }
    item
}

async fn exchange(State(state): State<Arc<StateData>>, request: Request) -> Response {
    if !request
        .headers()
        .get("x-hito-plugin-protocol")
        .is_some_and(|value| value == "1")
    {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let Ok(bytes) = axum::body::to_bytes(request.into_body(), 8192).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    let response = match (method, path.as_str()) {
        (Method::GET, "/v1/manifest") => {
            json!({"id": "org.hito.owned-resolution", "name": "Owned resolver", "protocol_version": 1, "capabilities": ["search", "sources"]})
        }
        (Method::POST, "/v1/recordings/detail")
            if [TARGET, CHOSEN, REMIX].contains(&body["recording_id"].as_str().unwrap_or("")) =>
        {
            state.details.fetch_add(1, Ordering::SeqCst);
            recording(body["recording_id"].as_str().unwrap())
        }
        (Method::POST, "/v1/sources") if body == json!({"recording_id": TARGET}) => {
            json!({"items": []})
        }
        (Method::POST, "/v1/sources") if body == json!({"recording_id": REMIX}) => {
            json!({"items": [{"id": "owned-remix-audio", "label": "Owned remix audio", "ready": true}]})
        }
        (Method::POST, "/v1/sources") if body == json!({"recording_id": CHOSEN}) => {
            json!({"items": [{"id": "owned-studio-audio", "label": "Owned studio audio", "ready": true}]})
        }
        (Method::POST, "/v1/search")
            if body
                == json!({"query": format!("{ARTIST} {TITLE}"), "cursor": null, "limit": 20}) =>
        {
            state.searches.fetch_add(1, Ordering::SeqCst);
            if state.without_exact_match {
                return Json(json!({"items": [recording(REMIX), recording("owned-live-recording")], "next_cursor": null})).into_response();
            }
            json!({"items": [recording("owned-live-recording"), recording("owned-other-performer"), recording("owned-remix-recording"), recording("owned-long-recording"), recording(CHOSEN)], "next_cursor": null})
        }
        _ => {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    Json(response).into_response()
}
