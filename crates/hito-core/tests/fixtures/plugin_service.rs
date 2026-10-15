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
    pub headers: HeaderMap,
    pub chunked: bool,
    pub gate: Option<Arc<Semaphore>>,
}

struct Fixture {
    reply: Mutex<Reply>,
    hits: AtomicUsize,
    entered: Semaphore,
    headers: Mutex<Vec<HeaderMap>>,
    bearer: Mutex<Option<String>>,
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
                headers: HeaderMap::from_iter([(
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/json"),
                )]),
                chunked: false,
                gate: None,
            }),
            hits: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            headers: Mutex::new(Vec::new()),
            bearer: Mutex::new(None),
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
    pub async fn require_bearer(&self, token: &str) {
        *self.state.bearer.lock().await = Some(format!("Bearer {token}"));
    }
    pub async fn response(
        &self,
        status: StatusCode,
        headers: HeaderMap,
        body: Vec<u8>,
        chunked: bool,
    ) {
        *self.state.reply.lock().await = Reply {
            status,
            headers,
            body,
            chunked,
            gate: None,
        };
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
    let authorized = state.bearer.lock().await.as_ref().is_none_or(|expected| {
        headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(expected)
    });
    state.headers.lock().await.push(headers);
    state.entered.add_permits(1);
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(gate) = reply.gate {
        gate.acquire().await.unwrap().forget();
    }
    let body = if reply.chunked {
        Body::from_stream(tokio_util::io::ReaderStream::new(std::io::Cursor::new(
            reply.body,
        )))
    } else {
        Body::from(reply.body)
    };
    (reply.status, reply.headers, body).into_response()
}

pub fn manifest() -> Value {
    json!({"id":"org.hito.test","name":"Fixture provider","protocol_version":1,"capabilities":["search","sources"]})
}
