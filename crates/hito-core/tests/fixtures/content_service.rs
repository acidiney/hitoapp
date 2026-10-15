#![allow(dead_code)]
use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock};

#[derive(Default)]
struct Fixture {
    replies: RwLock<HashMap<String, Value>>,
    requests: Mutex<Vec<(String, HeaderMap, Value)>>,
}
pub struct ProviderServer {
    pub url: String,
    state: Arc<Fixture>,
    task: tokio::task::JoinHandle<()>,
}
impl ProviderServer {
    pub async fn start() -> Self {
        let state = Arc::new(Fixture::default());
        let router = Router::new().route("/prefix/v1/manifest", get(|| async { Json(json!({"id":"org.hito.content-fixture","name":"Content fixture","protocol_version":1,"capabilities":["search","sources"]})) }))
            .route("/prefix/v1/search", post(exchange)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let task = tokio::spawn(async {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, state, task }
    }
    pub async fn reply(&self, path: &str, value: Value) {
        self.state.replies.write().await.insert(path.into(), value);
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
        self.task.abort();
    }
}
async fn exchange(
    State(state): State<Arc<Fixture>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let path = uri.path().trim_start_matches("/prefix/");
    state
        .requests
        .lock()
        .await
        .push((path.into(), headers, body));
    Json(
        state
            .replies
            .read()
            .await
            .get(path)
            .cloned()
            .unwrap_or(Value::Null),
    )
}
