#![allow(dead_code)]

use axum::http::{HeaderValue, StatusCode};
use axum::{
    Json, Router,
    extract::{Request, State},
    response::{IntoResponse, Response},
    routing::any,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::{sync::oneshot, task::JoinHandle};

pub const TOKEN: &str = "owned-lrclib-service-token";

pub struct Server {
    pub url: String,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Server {
    pub async fn start(app: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub async fn close(mut self) {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(std::time::Duration::from_secs(3), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub struct Upstream {
    pub reply: Arc<Mutex<Value>>,
    pub seen: Arc<Mutex<Vec<(String, String, String)>>>,
    pub response: Arc<Mutex<Option<Reply>>>,
}

#[derive(Clone)]
pub enum Reply {
    Retry(StatusCode, Vec<String>),
    Status(StatusCode),
    Typed(&'static str, Vec<u8>),
}

impl Upstream {
    pub async fn start(reply: Value) -> (Self, Server) {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let upstream = Self {
            reply: Arc::new(Mutex::new(reply)),
            seen: Arc::default(),
            response: Arc::default(),
        };
        let server = Server::start(
            Router::new()
                .fallback(any(receive))
                .with_state(upstream.clone()),
        )
        .await;
        (upstream, server)
    }
}

async fn receive(State(upstream): State<Upstream>, request: Request) -> Response {
    let agent = request
        .headers()
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    upstream.seen.lock().unwrap().push((
        request.method().to_string(),
        request.uri().to_string(),
        agent,
    ));
    if let Some(reply) = upstream.response.lock().unwrap().clone() {
        return match reply {
            Reply::Retry(status, values) => {
                let mut response = (status, "owned non-JSON edge rejection").into_response();
                for value in values {
                    response
                        .headers_mut()
                        .append("retry-after", HeaderValue::from_str(&value).unwrap());
                }
                response
            }
            Reply::Status(status) => status.into_response(),
            Reply::Typed(kind, body) => ([("content-type", kind)], body).into_response(),
        };
    }
    Json(upstream.reply.lock().unwrap().clone()).into_response()
}

pub fn authorized(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request
        .bearer_auth(TOKEN)
        .header("x-hito-plugin-protocol", "1")
}

pub fn record(id: u64) -> Value {
    serde_json::json!({"id": id, "trackName": "Owned river", "artistName": "Fixture ensemble", "albumName": "Owned collection", "duration": 123.45, "instrumental": false, "plainLyrics": "Synthetic river\nOwned horizon", "syncedLyrics": "[00:01.25]Synthetic river\n[00:03.50]Owned horizon\n[00:06.00]", "hasWordSync": false, "lyricsfile": ""})
}
