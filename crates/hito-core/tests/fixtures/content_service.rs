#![allow(dead_code)]
use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock, Semaphore, watch};

struct Fixture {
    range_chunk: RwLock<Option<usize>>,
    audio: RwLock<(StatusCode, HeaderMap, Vec<u8>)>,
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
            range_chunk: RwLock::new(None),
            audio: RwLock::new((StatusCode::NOT_FOUND, HeaderMap::new(), Vec::new())),
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
            .route("/prefix/v1/audio", get(audio))
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
    pub async fn audio(&self, status: StatusCode, headers: HeaderMap, bytes: Vec<u8>) {
        *self.state.audio.write().await = (status, headers, bytes);
    }
    pub async fn range_chunks(&self, maximum: usize) {
        *self.state.range_chunk.write().await = Some(maximum);
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
async fn audio(
    State(state): State<Arc<Fixture>>,
    axum::extract::Query(query): axum::extract::Query<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let range = headers
        .get("range")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    state
        .requests
        .lock()
        .await
        .push(("v1/audio".into(), headers, {
            let mut query = json!(query);
            query["_method"] = json!(method.as_str());
            query
        }));
    let (mut status, mut headers, mut bytes) = state.audio.read().await.clone();
    if let Some(maximum) = *state.range_chunk.read().await {
        headers.insert("accept-ranges", "bytes".parse().unwrap());
        if let Some(range) = range {
            let first: usize = range
                .strip_prefix("bytes=")
                .unwrap()
                .trim_end_matches('-')
                .parse()
                .unwrap();
            let total = bytes.len();
            if first >= total {
                headers.insert("content-range", format!("bytes */{total}").parse().unwrap());
                return (StatusCode::RANGE_NOT_SATISFIABLE, headers).into_response();
            }
            let end = (first + maximum).min(total);
            bytes = bytes[first..end].to_vec();
            status = StatusCode::PARTIAL_CONTENT;
            headers.insert(
                "content-range",
                format!("bytes {first}-{}/{total}", end - 1)
                    .parse()
                    .unwrap(),
            );
        }
        headers.insert("content-length", bytes.len().to_string().parse().unwrap());
    }
    if !wait_gate(&state, "v1/audio").await {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let gate = state.gates.read().await.get("audio-body").cloned();
    let tail = state.gates.read().await.get("audio-tail").cloned();
    if gate.is_some() || tail.is_some() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, reader) = tokio::io::duplex(65536);
        let mut closed = state.closed.clone();
        tokio::spawn(async move {
            let prefix = if tail.is_some() {
                bytes.len().min(64)
            } else {
                bytes.len()
            };
            tokio::select! {
                _ = closed.changed() => return,
                result = writer.write_all(&bytes[..prefix]) => if result.is_err() { return; },
            }
            if let Some(tail) = tail {
                tail.entered.add_permits(1);
                tokio::select! {
                    _ = closed.changed() => return,
                    result = tail.release.acquire() => result.unwrap().forget(),
                }
                tokio::select! {
                    _ = closed.changed() => return,
                    result = writer.write_all(&bytes[prefix..]) => if result.is_err() { return; },
                }
            }
            if let Some(gate) = gate {
                gate.entered.add_permits(1);
                tokio::select! {
                    _ = closed.changed() => {},
                    result = gate.release.acquire() => result.unwrap().forget(),
                }
            }
        });
        return (
            status,
            headers,
            axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(reader)),
        )
            .into_response();
    }
    (status, headers, bytes).into_response()
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
    if !wait_gate(&state, path).await {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(reply).into_response()
}
async fn wait_gate(state: &Fixture, path: &str) -> bool {
    let gate = state.gates.read().await.get(path).cloned();
    if let Some(gate) = gate {
        gate.entered.add_permits(1);
        let mut closed = state.closed.clone();
        tokio::select! {
            _ = closed.changed() => return false,
            permit = gate.release.acquire() => permit.unwrap().forget(),
        }
    }
    true
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
