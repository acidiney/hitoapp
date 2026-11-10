use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Faults {
    pushes: Arc<Mutex<Vec<Vec<u8>>>>,
    tokens: Arc<Mutex<Vec<String>>>,
    lose_ack: Arc<AtomicBool>,
    hold_pull: Arc<AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl Faults {
    pub fn lose_next_ack(&self) {
        self.lose_ack.store(true, Ordering::SeqCst);
    }
    pub fn hold_next_pull(&self) {
        self.hold_pull.store(true, Ordering::SeqCst);
    }
    pub async fn held(&self) {
        tokio::time::timeout(Duration::from_secs(3), self.entered.notified())
            .await
            .expect("actual Hito sync response was not held");
    }
    pub fn release(&self) {
        self.release.notify_one();
    }
    pub fn pushes(&self) -> Vec<Vec<u8>> {
        self.pushes.lock().unwrap().clone()
    }
    pub fn last_token(&self) -> String {
        self.tokens.lock().unwrap().last().unwrap().clone()
    }
}

pub async fn intercept(State(faults): State<Faults>, request: Request, next: Next) -> Response {
    let push = request.method() == "POST" && request.uri().path() == "/api/v1/sync";
    let pull = request.method() == "GET" && request.uri().path() == "/api/v1/sync";
    let login = request.method() == "POST" && request.uri().path() == "/api/v1/login";
    let request = if push {
        let (parts, body) = request.into_parts();
        let bytes = to_bytes(body, 262_144).await.unwrap();
        let _: hito_core::SyncBatch = serde_json::from_slice(&bytes).unwrap();
        faults.pushes.lock().unwrap().push(bytes.to_vec());
        Request::from_parts(parts, Body::from(bytes))
    } else {
        request
    };
    let response = next.run(request).await;
    if !response.status().is_success() {
        return response;
    }
    if push && faults.lose_ack.swap(false, Ordering::SeqCst) {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Body::empty())
            .unwrap();
    }
    if pull && faults.hold_pull.swap(false, Ordering::SeqCst) {
        faults.entered.notify_one();
        faults.release.notified().await;
    }
    if login {
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, 65_536).await.unwrap();
        let session: hito_core::AuthSession = serde_json::from_slice(&bytes).unwrap();
        faults.tokens.lock().unwrap().push(session.token);
        return Response::from_parts(parts, Body::from(bytes));
    }
    response
}
