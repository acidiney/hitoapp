#![allow(dead_code)]
use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock, Semaphore, watch};

struct Fixture {
    replies: RwLock<HashMap<String, Value>>,
    requests: Mutex<Vec<(String, HeaderMap, Value)>>,
    gates: RwLock<HashMap<String, Arc<Gate>>>,
    closed: watch::Receiver<bool>,
}
pub struct ProviderServer {
    pub url: String,
    state: Arc<Fixture>,
    task: tokio::task::JoinHandle<()>,
    closed: watch::Sender<bool>,
}
impl ProviderServer {
    pub async fn start() -> Self {
        let (closed, receiver) = watch::channel(false);
        let state = Arc::new(Fixture {
            replies: Default::default(),
            requests: Default::default(),
            gates: Default::default(),
            closed: receiver,
        });
        state.replies.write().await.insert("v1/manifest".into(), json!({"id":"org.hito.content-fixture","name":"Content fixture","protocol_version":1,"capabilities":["search","sources"]}));
        let router = Router::new()
            .route("/prefix/v1/manifest", get(manifest))
            .route("/prefix/v1/search", post(exchange))
            .route("/prefix/v1/recordings/detail", post(exchange))
            .route("/prefix/v1/sources", post(exchange))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let task = tokio::spawn(async {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            state,
            task,
            closed,
        }
    }
    pub async fn reply(&self, path: &str, value: Value) {
        self.state.replies.write().await.insert(path.into(), value);
    }
    pub async fn hold(&self, path: &str) -> Arc<Gate> {
        let gate = Arc::new(Gate {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        });
        self.state
            .gates
            .write()
            .await
            .insert(path.into(), gate.clone());
        gate
    }
    pub async fn requests(&self, path: &str) -> Vec<(HeaderMap, Value)> {
        self.state
            .requests
            .lock()
            .await
            .iter()
            .filter(|(route, _, _)| route == path)
            .map(|(_, headers, body)| (headers.clone(), body.clone()))
            .collect()
    }
}
impl Drop for ProviderServer {
    fn drop(&mut self) {
        let _ = self.closed.send(true);
        self.task.abort();
    }
}
async fn exchange(
    State(state): State<Arc<Fixture>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let path = uri.path().trim_start_matches("/prefix/");
    state
        .requests
        .lock()
        .await
        .push((path.into(), headers, body));
    let reply = state
        .replies
        .read()
        .await
        .get(path)
        .cloned()
        .unwrap_or(Value::Null);
    let gate = state.gates.read().await.get(path).cloned();
    if let Some(gate) = gate {
        gate.entered.add_permits(1);
        let mut closed = state.closed.clone();
        tokio::select! {
            _ = closed.changed() => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
            permit = gate.release.acquire() => permit.unwrap().forget(),
        }
    }
    Json(reply).into_response()
}
async fn manifest(State(state): State<Arc<Fixture>>) -> Json<Value> {
    Json(
        state
            .replies
            .read()
            .await
            .get("v1/manifest")
            .cloned()
            .unwrap(),
    )
}
pub struct Gate {
    entered: Semaphore,
    release: Semaphore,
}
impl Gate {
    pub async fn entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }
    pub fn release(&self) {
        self.release.add_permits(1);
    }
}
