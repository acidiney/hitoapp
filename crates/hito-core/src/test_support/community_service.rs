use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

pub const ADMIN_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const REPORTER_TOKEN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub struct Request {
    pub path: &'static str,
    pub authorization: Option<String>,
    pub body: Value,
}

pub struct CommunityServer {
    pub url: String,
    state: Arc<Fixture>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
struct Fixture {
    requests: Mutex<Vec<Request>>,
    reporters: Mutex<Vec<String>>,
    receipts: Mutex<Vec<(String, Uuid, Value)>>,
    provision_gate: Mutex<Option<Arc<Gate>>>,
    outcome_gate: Mutex<Option<Arc<Gate>>>,
}

struct Gate {
    entered: watch::Sender<bool>,
    released: watch::Sender<bool>,
}

pub struct HeldResponse(Arc<Gate>);

impl HeldResponse {
    pub async fn entered(&self) {
        let mut entered = self.0.entered.subscribe();
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            entered.wait_for(|entered| *entered),
        )
        .await
        .expect("The owned community request must reach its response barrier")
        .unwrap();
    }

    pub fn release(&self) {
        self.0.released.send_replace(true);
    }
}

impl Drop for HeldResponse {
    fn drop(&mut self) {
        self.release();
    }
}

impl CommunityServer {
    pub async fn hold_provision(&self) -> HeldResponse {
        hold(&self.state.provision_gate).await
    }

    pub async fn hold_outcome(&self) -> HeldResponse {
        hold(&self.state.outcome_gate).await
    }

    pub async fn start() -> Self {
        let state = Arc::new(Fixture::default());
        let router = Router::new()
            .route("/v1/admin/reporters", post(provision))
            .route("/v1/outcomes", post(outcome))
            .layer(axum::extract::DefaultBodyLimit::max(16_384))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, state, task }
    }

    pub async fn requests(&self) -> Vec<Request> {
        self.state
            .requests
            .lock()
            .await
            .iter()
            .map(|request| Request {
                path: request.path,
                authorization: request.authorization.clone(),
                body: request.body.clone(),
            })
            .collect()
    }

    pub async fn accepted_receipts(&self) -> Vec<Value> {
        self.state
            .receipts
            .lock()
            .await
            .iter()
            .map(|(_, _, receipt)| receipt.clone())
            .collect()
    }
}

impl Drop for CommunityServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn provision(
    State(state): State<Arc<Fixture>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let authorized = authorization(&headers) == Some(format!("Bearer {ADMIN_TOKEN}"));
    let valid = body == json!({});
    state.requests.lock().await.push(Request {
        path: "/v1/admin/reporters",
        authorization: authorization(&headers),
        body,
    });
    if !authorized || !valid {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "denied"})));
    }
    let mut reporters = state.reporters.lock().await;
    let token = if reporters.is_empty() {
        REPORTER_TOKEN.to_owned()
    } else {
        format!("{:064x}", reporters.len() + 12)
    };
    reporters.push(token.clone());
    drop(reporters);
    response_barrier(&state.provision_gate).await;
    (
        StatusCode::CREATED,
        Json(json!({"reporter_id": Uuid::new_v4(), "token": token})),
    )
}

async fn outcome(
    State(state): State<Arc<Fixture>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let token =
        authorization(&headers).and_then(|value| value.strip_prefix("Bearer ").map(str::to_owned));
    let reporters = state.reporters.lock().await;
    let authorized = token
        .as_ref()
        .is_some_and(|token| reporters.contains(token));
    drop(reporters);
    let submission = body
        .get("submission_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok());
    let valid = submission.is_some_and(|id| id.get_version_num() == 4)
        && body
            == json!({
                "submission_id": submission,
                "source": {"provider": "jamendo", "recording_id": "42"},
                "outcome": "successful_playback"
            });
    state.requests.lock().await.push(Request {
        path: "/v1/outcomes",
        authorization: authorization(&headers),
        body,
    });
    if !authorized || !valid {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "invalid"})),
        );
    }
    let token = token.unwrap();
    let submission = submission.unwrap();
    let mut receipts = state.receipts.lock().await;
    let (status, receipt) = if let Some((_, _, receipt)) = receipts
        .iter()
        .find(|(reporter, id, _)| reporter == &token && *id == submission)
    {
        (StatusCode::OK, receipt.clone())
    } else {
        let receipt = json!({"receipt_id": Uuid::new_v4(), "submission_id": submission, "status": "accepted"});
        receipts.push((token, submission, receipt.clone()));
        (StatusCode::CREATED, receipt)
    };
    drop(receipts);
    response_barrier(&state.outcome_gate).await;
    (status, Json(receipt))
}

async fn hold(pending: &Mutex<Option<Arc<Gate>>>) -> HeldResponse {
    let gate = Arc::new(Gate {
        entered: watch::channel(false).0,
        released: watch::channel(false).0,
    });
    let mut pending = pending.lock().await;
    assert!(
        pending.is_none(),
        "The owned service permits one pending response barrier"
    );
    *pending = Some(gate.clone());
    HeldResponse(gate)
}

async fn response_barrier(pending: &Mutex<Option<Arc<Gate>>>) {
    let gate = pending.lock().await.take();
    if let Some(gate) = gate {
        gate.entered.send_replace(true);
        let mut released = gate.released.subscribe();
        released.wait_for(|released| *released).await.unwrap();
    }
}

fn authorization(headers: &HeaderMap) -> Option<String> {
    let mut values = headers.get_all("authorization").iter();
    let value = values.next()?.to_str().ok()?.to_owned();
    values.next().is_none().then_some(value)
}
