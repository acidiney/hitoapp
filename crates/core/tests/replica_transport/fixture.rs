use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use futures_util::{StreamExt, stream};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

#[derive(Clone)]
struct Fixture {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    requests: Arc<Mutex<Vec<Received>>>,
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub header_delay: Duration,
    pub chunks: Vec<(Duration, Vec<u8>)>,
}

impl Reply {
    pub fn json(value: Value) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        Self {
            status: StatusCode::OK,
            headers,
            header_delay: Duration::ZERO,
            chunks: vec![(Duration::ZERO, serde_json::to_vec(&value).unwrap())],
        }
    }
    pub fn status(status: u16) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap(),
            ..Self::json(serde_json::json!({"error":"private-untrusted-error-body"}))
        }
    }
}

#[derive(Clone)]
pub struct Received {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

pub struct Server {
    pub endpoint: String,
    state: Fixture,
    stop: Option<oneshot::Sender<()>>,
    owner: Option<JoinHandle<()>>,
}

impl Server {
    pub async fn start(replies: Vec<Value>) -> Self {
        Self::scripted(replies.into_iter().map(Reply::json).collect()).await
    }
    pub async fn scripted(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = Fixture {
            replies: Arc::new(Mutex::new(replies.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let router = Router::new().fallback(receive).with_state(state.clone());
        let (stop, stopped) = oneshot::channel();
        let owner = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            endpoint,
            state,
            stop: Some(stop),
            owner: Some(owner),
        }
    }

    pub fn requests(&self) -> Vec<Received> {
        self.state.requests.lock().unwrap().clone()
    }

    pub async fn close(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(owner) = self.owner.take() {
            owner.await.unwrap();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(owner) = self.owner.take() {
            owner.abort();
        }
    }
}

async fn receive(State(state): State<Fixture>, request: Request) -> Response {
    let received = Received {
        method: request.method().to_string(),
        path: request.uri().to_string(),
        headers: request
            .headers()
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string()))
            .collect(),
        body: serde_json::from_slice(&to_bytes(request.into_body(), 1_048_576).await.unwrap())
            .unwrap_or(Value::Null),
    };
    state.requests.lock().unwrap().push(received);
    let reply = state
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or_else(|| Reply::status(500));
    tokio::time::sleep(reply.header_delay).await;
    let body = Body::from_stream(stream::iter(reply.chunks).then(async |(delay, bytes)| {
        tokio::time::sleep(delay).await;
        Ok::<_, std::io::Error>(bytes)
    }));
    let mut response = Response::new(body);
    *response.status_mut() = reply.status;
    *response.headers_mut() = reply.headers;
    response
}

pub fn account(id: &str, username: &str) -> Value {
    serde_json::to_value(hito_core::User {
        id: id.into(),
        username: username.into(),
        admin: false,
        display_name: None,
        avatar_version: None,
    })
    .unwrap()
}

pub fn playlist_batch(name: &str) -> hito_core::SyncBatch {
    hito_core::SyncBatch {
        device_id: "replica-client".into(),
        operations: vec![hito_core::Operation {
            id: uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
            action: hito_core::Action::PlaylistCreate {
                playlist_id: uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
                name: name.into(),
            },
        }],
    }
}
