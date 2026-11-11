use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::{RwLock, oneshot};
#[allow(dead_code)]
#[path = "timed_lyrics_body.rs"]
mod body;
pub use body::HeldBody;

struct Fixture {
    calls: Mutex<Vec<&'static str>>,
    capabilities: RwLock<Value>,
    metadata_revision: RwLock<String>,
    metadata_title: RwLock<String>,
    descriptor: RwLock<Option<Value>>,
    lyrics: RwLock<Option<Vec<u8>>>,
    held: Mutex<Option<(&'static str, Arc<HeldBody>)>>,
}
pub struct Provider {
    pub url: String,
    state: Arc<Fixture>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}
impl Provider {
    pub async fn start() -> Self {
        let state = Arc::new(Fixture {
            calls: Mutex::new(Vec::new()),
            capabilities: RwLock::new(json!(["search", "metadata", "lyrics"])),
            metadata_revision: RwLock::new("independent:1".into()),
            metadata_title: RwLock::new("Owned independent title".into()),
            descriptor: RwLock::new(Some(json!({"lyrics_version":2}))),
            lyrics: RwLock::new(None),
            held: Mutex::new(None),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/independent/", listener.local_addr().unwrap());
        let app = Router::new().fallback(exchange).with_state(state.clone());
        let (stop, receive) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = receive.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            state,
            stop: Some(stop),
            task,
        }
    }
    pub fn assert_calls(&self, expected: &[&str]) {
        assert_eq!(*self.state.calls.lock().unwrap(), expected);
    }
    pub fn calls(&self) -> Vec<&'static str> {
        self.state.calls.lock().unwrap().clone()
    }
    pub async fn descriptor(&self, value: Option<Value>) {
        *self.state.descriptor.write().await = value;
    }
    pub async fn lyrics(&self, value: Value) {
        self.lyrics_bytes(serde_json::to_vec(&value).unwrap()).await;
    }
    pub async fn lyrics_bytes(&self, value: Vec<u8>) {
        assert!(value.len() <= 524288);
        *self.state.lyrics.write().await = Some(value);
    }
    pub async fn metadata_revision(&self, value: &str) {
        *self.state.metadata_revision.write().await = value.into();
    }
    pub async fn metadata_title(&self, value: &str) {
        *self.state.metadata_title.write().await = value.into();
    }
    pub async fn capabilities(&self, value: Value) {
        *self.state.capabilities.write().await = value;
    }
    pub fn hold(&self, call: &'static str) -> Arc<HeldBody> {
        let mut held = self.state.held.lock().unwrap();
        assert!(held.is_none());
        let gate = HeldBody::new();
        *held = Some((call, gate.clone()));
        gate
    }
    pub fn release(&self) {
        if let Some((_, gate)) = self.state.held.lock().unwrap().take() {
            gate.release();
        }
    }
    pub async fn finish(mut self) {
        self.release();
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(std::time::Duration::from_secs(2), &mut self.task)
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn recording(id: &str, revision: &str, title: &str) -> Value {
    json!({"id":id,"title":title,"artist":"Owned independent artist","duration_ms":null,"release":null,"disc_number":null,"track_number":null,"revision":revision})
}
pub fn reference() -> Value {
    json!({"id":"independent:recording","revision":"independent:1"})
}
pub fn response(content: Value) -> Value {
    json!({"recording":reference(),"lyrics":{"id":"owned-independent-document","revision":"1","rendition_id":"owned-static-rendition","content":content}})
}
async fn exchange(State(state): State<Arc<Fixture>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    if parts.uri.path().ends_with("/v1/health") {
        return StatusCode::NOT_FOUND.into_response();
    }
    assert!(parts.uri.query().is_none());
    assert_eq!(parts.headers["x-hito-plugin-protocol"], "1");
    let bytes = to_bytes(body, 4096).await.unwrap();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let (call, value) = match (parts.method, parts.uri.path()) {
        (Method::GET, "/independent/v1/manifest") => (
            "manifest",
            json!({"id":"org.hito.owned-independent-lyrics","name":"Independent lyrics","protocol_version":1,"capabilities":state.capabilities.read().await.clone()}),
        ),
        (Method::POST, "/independent/v1/search") if body["query"] == "Explicit private search" => (
            "search",
            json!({"items":[recording("independent:recording",&state.metadata_revision.read().await,&state.metadata_title.read().await)],"next_cursor":null}),
        ),
        (Method::POST, "/independent/v1/recordings/metadata")
            if body["recording_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("independent:recording")) =>
        {
            (
                "metadata",
                json!({"recording":recording(body["recording_id"].as_str().unwrap(),&state.metadata_revision.read().await,&state.metadata_title.read().await)}),
            )
        }
        (Method::GET, "/independent/v2/lyrics/capabilities") => {
            let value = state.descriptor.read().await.clone();
            let Some(value) = value else {
                state.calls.lock().unwrap().push("v2_missing");
                return StatusCode::NOT_FOUND.into_response();
            };
            ("v2_descriptor", value)
        }
        (Method::GET, "/independent/v1/lyrics/capabilities") => {
            ("v1_descriptor", json!({"lyrics_version":1}))
        }
        (Method::POST, "/independent/v2/lyrics") if body == json!({"recording":reference()}) => (
            "v2_lyrics",
            response(json!({"kind":"plain","lines":["Owned untimed lyric text"]})),
        ),
        (Method::POST, "/independent/v1/lyrics") if body == json!({"recording":reference()}) => {
            ("v1_lyrics", json!({"recording":reference(),"lyrics":null}))
        }
        _ => panic!("Independent lyric provider received an unexpected operation"),
    };
    state.calls.lock().unwrap().push(call);
    let bytes = if call == "v2_lyrics" || call == "v1_lyrics" {
        state
            .lyrics
            .read()
            .await
            .clone()
            .unwrap_or_else(|| serde_json::to_vec(&value).unwrap())
    } else {
        serde_json::to_vec(&value).unwrap()
    };
    let held = state
        .held
        .lock()
        .unwrap()
        .as_ref()
        .filter(|(selected, _)| *selected == call)
        .map(|(_, gate)| gate.clone());
    if let Some(gate) = held {
        return Response::builder()
            .header("content-type", "application/json")
            .body(gate.body(bytes))
            .unwrap();
    }
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return body::chunked_json(bytes),
    };
    if bytes.len() > 262144 {
        return body::chunked_json(bytes);
    }
    Json(value).into_response()
}
