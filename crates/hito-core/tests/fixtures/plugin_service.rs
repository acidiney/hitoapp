#![allow(dead_code)]

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone)]
pub struct Reply {
    pub body: Vec<u8>,
    pub status: StatusCode,
    pub gate: Option<Arc<Semaphore>>,
}

struct Fixture {
    reply: Mutex<Reply>,
    hits: AtomicUsize,
    entered: Semaphore,
    headers: Mutex<Vec<HeaderMap>>,
}

pub struct PluginServer {
    pub url: String,
    state: Arc<Fixture>,
    task: tokio::task::JoinHandle<()>,
}

impl PluginServer {
    pub async fn start() -> Self {
        let state = Arc::new(Fixture {
            reply: Mutex::new(Reply {
                body: serde_json::to_vec(&manifest()).unwrap(),
                status: StatusCode::OK,
                gate: None,
            }),
            hits: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            headers: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/prefix/v1/manifest", get(respond))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let task = tokio::spawn(async {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, state, task }
    }
    pub async fn reply(&self, body: Value) {
        self.state.reply.lock().await.body = serde_json::to_vec(&body).unwrap();
    }
    pub async fn hold(&self) -> Arc<Semaphore> {
        let gate = Arc::new(Semaphore::new(0));
        self.state.reply.lock().await.gate = Some(gate.clone());
        gate
    }
    pub async fn entered(&self) {
        self.state.entered.acquire().await.unwrap().forget();
    }
    pub fn hits(&self) -> usize {
        self.state.hits.load(Ordering::SeqCst)
    }
    pub async fn headers(&self) -> Vec<HeaderMap> {
        self.state.headers.lock().await.clone()
    }
}

impl Drop for PluginServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn respond(State(state): State<Arc<Fixture>>, headers: HeaderMap) -> Response {
    let reply = state.reply.lock().await.clone();
    state.hits.fetch_add(1, Ordering::SeqCst);
    state.headers.lock().await.push(headers);
    state.entered.add_permits(1);
    if let Some(gate) = reply.gate {
        gate.acquire().await.unwrap().forget();
    }
    (
        reply.status,
        [("content-type", "application/json")],
        Body::from(reply.body),
    )
        .into_response()
}

pub fn manifest() -> Value {
    json!({"id":"org.hito.test","name":"Fixture provider","protocol_version":1,"capabilities":["search","sources"]})
}
