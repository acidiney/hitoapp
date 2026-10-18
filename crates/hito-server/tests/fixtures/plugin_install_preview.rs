use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

pub const TOKEN: &str = "private-http-install-preview-token-sentinel";

struct StateData {
    manifest: Mutex<Value>,
    requests: Mutex<Vec<(String, HeaderMap)>>,
}

pub struct Provider {
    pub url: String,
    address: SocketAddr,
    state: Arc<StateData>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl Provider {
    pub async fn start(manifest: Value) -> Self {
        let state = Arc::new(StateData {
            manifest: Mutex::new(manifest),
            requests: Mutex::new(Vec::new()),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().fallback(respond).with_state(state.clone());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        Self {
            url: format!("http://{address}/prefix/"),
            address,
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub async fn reply(&self, manifest: Value) {
        *self.state.manifest.lock().await = manifest;
    }

    pub async fn assert_manifest_requests(&self, authorization: &[bool]) {
        let requests = self.state.requests.lock().await;
        assert_eq!(requests.len(), authorization.len());
        for ((path, headers), authenticated) in requests.iter().zip(authorization) {
            assert_eq!(path, "/prefix/v1/manifest");
            assert!(headers["x-hito-plugin-protocol"] == "1");
            if *authenticated {
                assert!(headers["authorization"] == format!("Bearer {TOKEN}"));
            } else {
                assert!(headers.get("authorization").is_none());
            }
        }
    }

    pub async fn finish(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => {
                matches!(result, Ok(true)) && TcpStream::connect(self.address).await.is_err()
            }
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

async fn respond(State(state): State<Arc<StateData>>, request: Request) -> Response {
    let valid = request.method() == "GET" && request.uri().path() == "/prefix/v1/manifest";
    let authorized = request
        .headers()
        .get("authorization")
        .is_some_and(|header| header == format!("Bearer {TOKEN}").as_str());
    state
        .requests
        .lock()
        .await
        .push((request.uri().path().to_owned(), request.headers().clone()));
    if !valid {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(state.manifest.lock().await.clone()).into_response()
}
