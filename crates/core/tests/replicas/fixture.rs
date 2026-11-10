use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use hito_core::{Actor, ConnectReplica, Credentials, Services, SyncBatch};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::{net::TcpListener, task::JoinHandle};

pub const REMOTE_ACCOUNT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub const TOKEN: &str = "1111111111111111111111111111111111111111111111111111111111111111";

pub fn user() -> Value {
    json!({"id": REMOTE_ACCOUNT, "username": "remote-listener", "admin": false,
        "display_name": null, "avatar_version": null})
}

#[derive(Clone, Default)]
struct Ledger {
    rows: Arc<Mutex<Vec<Value>>>,
    lose_ack: Arc<AtomicBool>,
    fail_pull: Arc<AtomicBool>,
    hold_pull: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    reply: Arc<Mutex<Option<Value>>>,
}

pub struct RemoteServer {
    pub endpoint: String,
    task: JoinHandle<()>,
    ledger: Ledger,
}

impl RemoteServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let ledger = Ledger::default();
        let app = Router::new()
            .route(
                "/api/v1/login",
                post(|| async { Json(json!({"token": TOKEN, "user": user()})) }),
            )
            .route("/api/v1/me", get(|| async { Json(user()) }))
            .route("/api/v1/sync", post(push).get(pull))
            .with_state(ledger.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            endpoint,
            task,
            ledger,
        }
    }

    pub fn lose_next_ack(&self) {
        self.ledger.lose_ack.store(true, Ordering::SeqCst);
    }
    pub fn fail_next_pull(&self) {
        self.ledger.fail_pull.store(true, Ordering::SeqCst);
    }
    pub fn hold_next_pull(&self) {
        self.ledger.hold_pull.store(true, Ordering::SeqCst);
    }
    pub async fn wait_for_pull(&self) {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.ledger.entered.notified(),
        )
        .await
        .unwrap();
    }
    pub fn release_pull(&self) {
        self.ledger.release.notify_one();
    }
    pub fn reply_next_pull(&self, reply: Value) {
        *self.ledger.reply.lock().unwrap() = Some(reply);
    }
    pub fn event_count(&self) -> usize {
        self.ledger.rows.lock().unwrap().len()
    }

    pub fn request(&self) -> ConnectReplica {
        ConnectReplica {
            endpoint: self.endpoint.clone(),
            credentials: Credentials {
                username: "remote-listener".into(),
                password: "owned-remote-password".into(),
            },
        }
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
        && headers
            .get("x-hito-account")
            .is_some_and(|value| value == REMOTE_ACCOUNT)
}

async fn push(
    State(ledger): State<Ledger>,
    headers: HeaderMap,
    Json(batch): Json<SyncBatch>,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut rows = ledger.rows.lock().unwrap();
    let mut acknowledged = Vec::new();
    for operation in batch.operations {
        let payload = serde_json::to_value(operation).unwrap();
        if let Some(previous) = rows
            .iter()
            .find(|row| row["operation"]["id"] == payload["id"])
        {
            if previous["operation"] != payload || previous["device_id"] != batch.device_id {
                return Err(StatusCode::CONFLICT);
            }
        } else {
            let sequence = rows.len() + 1;
            rows.push(
                json!({"sequence": sequence, "device_id": batch.device_id, "operation": payload}),
            );
        }
        acknowledged.push(payload["id"].clone());
    }
    if ledger.lose_ack.swap(false, Ordering::SeqCst) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(Json(json!({"acknowledged": acknowledged})))
}

#[derive(Deserialize)]
struct Cursor {
    #[serde(default)]
    cursor: i64,
}

async fn pull(
    State(ledger): State<Ledger>,
    headers: HeaderMap,
    Query(query): Query<Cursor>,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    if ledger.hold_pull.swap(false, Ordering::SeqCst) {
        ledger.entered.notify_one();
        ledger.release.notified().await;
    }
    if ledger.fail_pull.swap(false, Ordering::SeqCst) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    if let Some(reply) = ledger.reply.lock().unwrap().take() {
        return Ok(Json(reply));
    }
    let rows = ledger.rows.lock().unwrap();
    let operations: Vec<_> = rows
        .iter()
        .filter(|row| row["sequence"].as_i64().unwrap() > query.cursor)
        .cloned()
        .collect();
    let cursor = operations
        .last()
        .map_or(query.cursor, |row| row["sequence"].as_i64().unwrap());
    Ok(Json(
        json!({"operations": operations, "cursor": cursor, "has_more": false}),
    ))
}

pub async fn local() -> (tempfile::TempDir, Services, Actor) {
    let (directory, services, actor, _, _) =
        hito_core::test_support::support::library("replica-local-host").await;
    (directory, services, actor)
}
