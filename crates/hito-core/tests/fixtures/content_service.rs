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
    offline: RwLock<bool>,
    playlist_pages: RwLock<HashMap<Option<String>, Value>>,
    range_chunk: RwLock<Option<usize>>,
    held_range: RwLock<Option<std::ops::Range<usize>>>,
    broken_tail: RwLock<Option<usize>>,
    audio: RwLock<(StatusCode, HeaderMap, Vec<u8>)>,
    replies: RwLock<HashMap<String, Value>>,
    recording_replies: RwLock<HashMap<(String, String), Value>>,
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
            offline: RwLock::new(false),
            playlist_pages: Default::default(),
            range_chunk: RwLock::new(None),
            held_range: RwLock::new(None),
            broken_tail: RwLock::new(None),
            audio: RwLock::new((StatusCode::NOT_FOUND, HeaderMap::new(), Vec::new())),
            replies: Default::default(),
            recording_replies: Default::default(),
            requests: Default::default(),
            gates: Default::default(),
            closed: receiver,
        });
        state.replies.write().await.insert("v1/manifest".into(), json!({"id":"org.hito.content-fixture","name":"Content fixture","protocol_version":1,"capabilities":["search","sources"]}));
        let router = Router::new()
            .route("/prefix/v1/manifest", get(manifest))
            .route("/prefix/v1/search", post(exchange))
            .route("/prefix/v1/recordings/detail", post(exchange))
            .route("/prefix/v1/recordings/metadata", post(exchange))
            .route("/prefix/v1/playlists/detail", post(exchange))
            .route("/prefix/v1/playlists/items", post(exchange))
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
    pub async fn recording_reply(&self, path: &str, recording: &str, value: Value) {
        assert!(matches!(
            path,
            "v1/recordings/detail" | "v1/recordings/metadata" | "v1/sources"
        ));
        self.state
            .recording_replies
            .write()
            .await
            .insert((path.into(), recording.into()), value);
    }
    pub async fn offline(&self) {
        *self.state.offline.write().await = true;
    }
    pub async fn playlist_pages(&self, pages: Vec<(Option<String>, Value)>) {
        *self.state.playlist_pages.write().await = pages.into_iter().collect();
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
    pub async fn hold_range(&self, range: std::ops::Range<usize>) -> Arc<Gate> {
        *self.state.held_range.write().await = Some(range);
        self.hold("audio-range").await
    }
    pub async fn hold_broken_tail(&self, prefix: usize) -> Arc<Gate> {
        *self.state.broken_tail.write().await = Some(prefix);
        self.hold("audio-tail").await
    }
    pub async fn clear_audio_tail_fault(&self) {
        *self.state.broken_tail.write().await = None;
        if let Some(gate) = self.state.gates.write().await.remove("audio-tail") {
            gate.open();
        }
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
    pub async fn shutdown(mut self) {
        let _ = self.closed.send(true);
        self.task.abort();
        let _ = (&mut self.task).await;
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
    if *state.offline.read().await {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let first = range
        .as_deref()
        .and_then(|range| range.strip_prefix("bytes="))
        .and_then(|range| range.split_once('-'))
        .and_then(|(first, _)| first.parse::<usize>().ok());
    let (mut status, mut headers, mut bytes) = state.audio.read().await.clone();
    if let Some(maximum) = *state.range_chunk.read().await {
        headers.insert("accept-ranges", "bytes".parse().unwrap());
        if let Some(first) = first {
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
    let held_range = state.held_range.read().await.clone();
    if first.is_some_and(|first| {
        held_range
            .as_ref()
            .is_none_or(|range| range.contains(&first))
    }) && !wait_gate(&state, "audio-range").await
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let gate = state.gates.read().await.get("audio-body").cloned();
    let tail = state.gates.read().await.get("audio-tail").cloned();
    let broken_tail = *state.broken_tail.read().await;
    if gate.is_some() || tail.is_some() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, reader) = tokio::io::duplex(65536);
        let mut closed = state.closed.clone();
        tokio::spawn(async move {
            let prefix = if tail.is_some() {
                bytes.len().min(broken_tail.unwrap_or(64))
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
                    result = tail.release.acquire() => if let Ok(permit) = result { permit.forget(); },
                }
                if broken_tail.is_none() {
                    tokio::select! {
                        _ = closed.changed() => return,
                        result = writer.write_all(&bytes[prefix..]) => if result.is_err() { return; },
                    }
                }
            }
            if let Some(gate) = gate {
                gate.entered.add_permits(1);
                tokio::select! {
                    _ = closed.changed() => {},
                    result = gate.release.acquire() => if let Ok(permit) = result { permit.forget(); },
                }
            }
        });
        let body = if broken_tail.is_some() {
            axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(BrokenTail(reader)))
        } else {
            axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(reader))
        };
        return (status, headers, body).into_response();
    }
    (status, headers, bytes).into_response()
}

struct BrokenTail(tokio::io::DuplexStream);
impl tokio::io::AsyncRead for BrokenTail {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buffer.filled().len();
        match std::pin::Pin::new(&mut self.0).poll_read(cx, buffer) {
            std::task::Poll::Ready(Ok(())) if buffer.filled().len() == before => {
                std::task::Poll::Ready(Err(std::io::Error::other("Fixture interrupted body")))
            }
            result => result,
        }
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
    let playlist_body = (path == "v1/playlists/items").then(|| body.clone());
    let recording_body = matches!(
        path,
        "v1/recordings/detail" | "v1/recordings/metadata" | "v1/sources"
    )
    .then(|| body.clone());
    state
        .requests
        .lock()
        .await
        .push((path.into(), headers, body));
    if *state.offline.read().await {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let page = if let Some(body) = playlist_body {
        let pages = state.playlist_pages.read().await;
        if pages.is_empty() {
            None
        } else {
            let Ok(request) = serde_json::from_value::<hito_plugin_sdk::PlaylistItemsRequest>(body)
            else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if request.validate().is_err() {
                return StatusCode::BAD_REQUEST.into_response();
            }
            let Some(page) = pages.get(&request.cursor) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if page["playlist_id"].as_str() != Some(request.playlist_id.as_str())
                || page["snapshot"].as_str() != Some(request.snapshot.as_str())
                || page["items"]
                    .as_array()
                    .is_none_or(|items| items.len() > usize::from(request.limit))
            {
                return StatusCode::BAD_REQUEST.into_response();
            }
            Some(page.clone())
        }
    } else {
        None
    };
    let recording_reply = if let Some(body) = recording_body {
        let replies = state.recording_replies.read().await;
        if replies.keys().any(|(route, _)| route == path) {
            let Ok(request) = serde_json::from_value::<hito_plugin_sdk::RecordingRequest>(body)
            else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if request.validate().is_err() {
                return StatusCode::BAD_REQUEST.into_response();
            }
            replies
                .get(&(path.to_owned(), request.recording_id))
                .cloned()
        } else {
            None
        }
    } else {
        None
    };
    let reply = if let Some(reply) = page.or(recording_reply) {
        reply
    } else {
        state
            .replies
            .read()
            .await
            .get(path)
            .cloned()
            .unwrap_or(Value::Null)
    };
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
            permit = gate.release.acquire() => if let Ok(permit) = permit { permit.forget(); },
        }
    }
    true
}
async fn manifest(State(state): State<Arc<Fixture>>, headers: HeaderMap) -> Response {
    state
        .requests
        .lock()
        .await
        .push(("v1/manifest".into(), headers, Value::Null));
    if *state.offline.read().await {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(
        state
            .replies
            .read()
            .await
            .get("v1/manifest")
            .cloned()
            .unwrap(),
    )
    .into_response()
}
pub struct Gate {
    entered: Semaphore,
    release: Semaphore,
}
impl Gate {
    pub fn open(&self) {
        self.release.close();
    }
    pub async fn entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }
    pub fn release(&self) {
        self.release.add_permits(1);
    }
}
