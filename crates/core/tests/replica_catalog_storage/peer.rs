use super::{
    export::{ACCOUNT, Export, LIBRARY, PAGE_SIZE, SNAPSHOT},
    fixture::{Bound, Case},
};
use axum::{
    Json, Router,
    extract::{Path as RoutePath, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use hito_core::{
    AuthSession, BeginCatalogSnapshot, CatalogSnapshot, SyncBatch, SyncEvent, SyncPage,
    SyncReceipt, User,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    net::TcpListener,
    sync::{Notify, oneshot},
    task::JoinHandle,
};
use uuid::Uuid;
const TOKEN: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[derive(Clone)]
struct PeerState {
    export: Arc<Mutex<Export>>,
    events: Arc<Mutex<Vec<SyncEvent>>>,
    pushes: Arc<Mutex<Vec<Value>>>,
    cursors: Arc<Mutex<Vec<i64>>>,
    pub pages: Arc<AtomicUsize>,
    pub bytes: Arc<AtomicUsize>,
    hold: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

pub struct Remote {
    pub endpoint: String,
    state: PeerState,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Remote {
    pub async fn start(recordings: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = PeerState {
            export: Arc::new(Mutex::new(Export {
                id: SNAPSHOT,
                recordings,
            })),
            events: Default::default(),
            pushes: Default::default(),
            cursors: Default::default(),
            pages: Default::default(),
            bytes: Default::default(),
            hold: Arc::new(AtomicUsize::new(usize::MAX)),
            entered: Default::default(),
            release: Default::default(),
        };
        let mut remote = Self {
            endpoint,
            state,
            stop: None,
            task: None,
        };
        remote.serve(listener);
        remote
    }

    fn serve(&mut self, listener: TcpListener) {
        let app = Router::new()
            .route(
                "/api/v1/login",
                post(|| async {
                    Json(AuthSession {
                        token: TOKEN.into(),
                        user: remote_user(),
                    })
                }),
            )
            .route("/api/v1/me", get(|| async { Json(remote_user()) }))
            .route("/api/v1/sync", post(push).get(pull))
            .route("/api/v1/catalog/snapshots", post(begin))
            .route("/api/v1/catalog/snapshots/{id}/pages/{ordinal}", get(page))
            .route(
                "/api/v1/catalog/snapshots/{id}",
                axum::routing::delete(|| async { StatusCode::NO_CONTENT }),
            )
            .with_state(self.state.clone());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        self.stop = Some(stop);
        self.task = Some(task);
    }

    pub async fn pause(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        self.task.take().unwrap().await.unwrap();
    }

    pub async fn resume(&mut self) {
        let address = self.endpoint.strip_prefix("http://").unwrap();
        self.serve(TcpListener::bind(address).await.unwrap());
    }

    pub fn replace(&self, export: Export) {
        *self.state.export.lock().unwrap() = export;
        self.state.pages.store(0, Ordering::SeqCst);
        self.state.bytes.store(0, Ordering::SeqCst);
    }

    pub fn hold_page(&self, ordinal: usize) {
        self.state.hold.store(ordinal, Ordering::SeqCst);
    }
    pub async fn wait_for_page(&self) {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.state.entered.notified(),
        )
        .await
        .unwrap();
    }
    pub fn release_page(&self) {
        self.state.release.notify_one();
    }
    pub fn counts(&self) -> (usize, usize) {
        (
            self.state.pages.load(Ordering::SeqCst),
            self.state.bytes.load(Ordering::SeqCst),
        )
    }

    pub async fn stop(mut self) {
        self.pause().await;
    }

    pub async fn assert_pending_and_cursor(&self, case: &Case, bound: &Bound) {
        let outcome = case
            .services
            .replicas
            .synchronize(&case.actor, &bound.session)
            .await
            .unwrap();
        assert_eq!(
            (
                outcome.pushed,
                outcome.received,
                outcome.pending,
                outcome.cursor
            ),
            (1, 1, 0, 2)
        );
        let pushes = self.state.pushes.lock().unwrap();
        assert_eq!(pushes.len(), 2);
        assert_eq!(pushes[0]["device_id"], bound.client_id);
        assert_eq!(pushes[1]["device_id"], pushes[0]["device_id"]);
        assert_eq!(
            pushes[1]["operations"],
            serde_json::json!([super::fixture::pending()])
        );
        assert_eq!(*self.state.cursors.lock().unwrap(), vec![0, 1]);
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn remote_user() -> User {
    User {
        id: ACCOUNT.into(),
        username: "owned-peer".into(),
        admin: false,
        display_name: None,
        avatar_version: None,
    }
}
fn authorize(headers: &HeaderMap) -> Result<(), StatusCode> {
    if headers
        .get("authorization")
        .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
        && headers
            .get("x-hito-account")
            .is_some_and(|value| value == ACCOUNT)
    {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
async fn begin(
    State(state): State<PeerState>,
    headers: HeaderMap,
    Json(request): Json<BeginCatalogSnapshot>,
) -> Result<Json<CatalogSnapshot>, StatusCode> {
    authorize(&headers)?;
    if request.library_id != LIBRARY {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(state.export.lock().unwrap().metadata()))
}
async fn page(
    State(state): State<PeerState>,
    headers: HeaderMap,
    RoutePath((id, ordinal)): RoutePath<(Uuid, u64)>,
) -> Result<impl IntoResponse, StatusCode> {
    authorize(&headers)?;
    let export = *state.export.lock().unwrap();
    if id != export.id || ordinal >= export.metadata().page_count {
        return Err(StatusCode::NOT_FOUND);
    }
    if state
        .hold
        .compare_exchange(
            ordinal as usize,
            usize::MAX,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
    {
        state.entered.notify_one();
        state.release.notified().await;
    }
    let page = export.page(ordinal);
    assert!(page.recordings.len() <= PAGE_SIZE);
    for fact in &page.recordings {
        assert!(serde_json::to_vec(fact).unwrap().len() <= 64 * 1024);
    }
    let bytes = serde_json::to_vec(&page).unwrap();
    assert!(bytes.len() <= 256 * 1024);
    state.pages.fetch_add(1, Ordering::SeqCst);
    state.bytes.fetch_add(bytes.len(), Ordering::SeqCst);
    Ok(([("content-type", "application/json")], bytes))
}
async fn push(
    State(state): State<PeerState>,
    headers: HeaderMap,
    Json(batch): Json<SyncBatch>,
) -> Result<Json<SyncReceipt>, StatusCode> {
    authorize(&headers)?;
    state
        .pushes
        .lock()
        .unwrap()
        .push(serde_json::to_value(&batch).unwrap());
    let mut events = state.events.lock().unwrap();
    let mut acknowledged = Vec::new();
    for operation in batch.operations {
        acknowledged.push(operation.id);
        let sequence = events.len() as i64 + 1;
        events.push(SyncEvent::Operation {
            sequence,
            device_id: batch.device_id.clone(),
            operation,
        });
    }
    Ok(Json(SyncReceipt { acknowledged }))
}
#[derive(Deserialize)]
struct Cursor {
    cursor: i64,
}
async fn pull(
    State(state): State<PeerState>,
    headers: HeaderMap,
    Query(query): Query<Cursor>,
) -> Result<Json<SyncPage>, StatusCode> {
    authorize(&headers)?;
    state.cursors.lock().unwrap().push(query.cursor);
    let events = state.events.lock().unwrap();
    let operations = events
        .iter()
        .filter(|event| event.sequence() > query.cursor)
        .map(|event| serde_json::from_value(serde_json::to_value(event).unwrap()).unwrap())
        .collect();
    Ok(Json(SyncPage {
        operations,
        cursor: events.len() as i64,
        has_more: false,
    }))
}
