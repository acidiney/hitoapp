use axum::{
    Json, Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock, oneshot, watch};

pub struct Observation {
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Value,
}

#[derive(Default)]
struct Fixture {
    requests: Mutex<Vec<Observation>>,
    response: RwLock<Value>,
    held: Mutex<Option<Arc<HeldBody>>>,
}

pub struct RankingServer {
    pub url: String,
    state: Arc<Fixture>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl RankingServer {
    pub async fn start() -> Self {
        let state = Arc::new(Fixture::default());
        let router = Router::new().fallback(exchange).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            state,
            stop: Some(stop),
            task,
        }
    }

    pub async fn reply(&self, response: Value) {
        assert!(serde_json::to_vec(&response).unwrap().len() < 4096);
        *self.state.response.write().await = response;
    }

    pub async fn assert_no_requests(&self) {
        assert!(
            self.state.requests.lock().await.is_empty(),
            "Disabled ranking and preference saves must not contact the community service"
        );
    }

    pub async fn assert_one_public_request(&self, expected: Value) {
        self.assert_public_requests(&[expected]).await;
    }

    pub async fn assert_public_requests(&self, expected: &[Value]) {
        let requests = self.state.requests.lock().await;
        assert_eq!(requests.len(), expected.len());
        for (request, expected) in requests.iter().zip(expected) {
            assert!(request.method == Method::POST && request.path == "/v1/rankings");
            assert!(
                request.body == *expected,
                "Only exact canonical public source references may leave Hito"
            );
            for forbidden in [
                "authorization",
                "cookie",
                "x-hito-account",
                "x-hito-session",
                "referer",
            ] {
                assert!(!request.headers.contains_key(forbidden));
            }
            assert!(request.headers["content-type"] == "application/json");
        }
    }

    pub async fn hold(&self) -> Arc<HeldBody> {
        let mut held = self.state.held.lock().await;
        assert!(held.is_none());
        let gate = Arc::new(HeldBody {
            entered: watch::channel(false).0,
            retired: watch::channel(false).0,
            released: watch::channel(false).0,
        });
        *held = Some(gate.clone());
        gate
    }

    pub async fn release(&self) {
        if let Some(gate) = self.state.held.lock().await.take() {
            gate.released.send_replace(true);
        }
    }

    pub async fn close(mut self) {
        self.release().await;
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(2), &mut self.task)
            .await
            .expect("The owned ranking fixture must stop within its deadline")
            .expect("The owned ranking fixture must join shutdown");
    }
}

impl Drop for RankingServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct HeldBody {
    entered: watch::Sender<bool>,
    retired: watch::Sender<bool>,
    released: watch::Sender<bool>,
}

struct Retire(Arc<HeldBody>);

impl Drop for Retire {
    fn drop(&mut self) {
        self.0.retired.send_replace(true);
    }
}

impl HeldBody {
    pub async fn entered(&self) {
        let mut entered = self.entered.subscribe();
        drop(entered.wait_for(|value| *value).await.unwrap());
    }

    pub async fn retired(&self) {
        let mut retired = self.retired.subscribe();
        drop(retired.wait_for(|value| *value).await.unwrap());
    }

    pub fn released(&self) -> bool {
        *self.released.borrow()
    }

    fn body(self: Arc<Self>, bytes: Vec<u8>) -> Body {
        assert!((2..=4096).contains(&bytes.len()));
        Body::from_stream(futures_util::stream::unfold(
            Some((Retire(self), bytes, false)),
            |state| async move {
                let (guard, bytes, started) = state?;
                if !started {
                    guard.0.entered.send_replace(true);
                    return Some((
                        Ok::<_, Infallible>(Bytes::copy_from_slice(&bytes[..1])),
                        Some((guard, bytes, true)),
                    ));
                }
                let mut released = guard.0.released.subscribe();
                drop(released.wait_for(|value| *value).await.unwrap());
                Some((Ok(Bytes::copy_from_slice(&bytes[1..])), None))
            },
        ))
    }
}

async fn exchange(State(state): State<Arc<Fixture>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = tokio::time::timeout(Duration::from_secs(1), to_bytes(body, 4096))
        .await
        .expect("The owned ranking request must have a bounded body deadline")
        .expect("The owned ranking request must stay within its byte limit");
    let mut requests = state.requests.lock().await;
    assert!(requests.len() < 4, "Community requests must remain bounded");
    requests.push(Observation {
        method: parts.method,
        path: parts.uri.to_string(),
        headers: parts.headers,
        body: serde_json::from_slice(&bytes).unwrap(),
    });
    drop(requests);
    let value = state.response.read().await.clone();
    if let Some(gate) = state.held.lock().await.clone() {
        return Response::builder()
            .header("content-type", "application/json")
            .body(gate.body(serde_json::to_vec(&value).unwrap()))
            .unwrap();
    }
    Json(value).into_response()
}
