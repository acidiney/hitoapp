use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, CATALOG_EDITION_VERSION,
    CATALOG_VERSION, Capability, CatalogCapabilities, CatalogEditionCapabilities,
    CatalogEditionOperation, CatalogEditionSummary, CatalogEditionTrackList, CatalogOperation,
    EditionTrackListRequest, Manifest, PROTOCOL_VERSION, ReleaseGroupEditionsPage,
    ReleaseGroupEditionsRequest,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Semaphore, oneshot},
    task::{JoinHandle, JoinSet},
};

pub const ARTIST_ID: &str = "artist:owned/演奏?credit=lead";
pub const GROUP_ID: &str = "group:owned/á?album=一&credit=guest";
pub const BONUS_ID: &str = "edition:owned/bonus?pressing=二";
pub const PROVIDER_CURSOR: &str = "cursor:owned/次?edition=2&signature=private";
const STANDARD_ID: &str = "edition:owned/standard?pressing=一";
const RECORDING_ID: &str = "recording:owned/演奏?take=studio";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedRequest {
    pub method: Method,
    pub path: String,
    pub body: Value,
}

#[derive(Default)]
struct Requests {
    listing_enabled: AtomicBool,
    observed: Mutex<Vec<ObservedRequest>>,
    unexpected: AtomicUsize,
    held: Mutex<Option<(CatalogEditionOperation, Arc<EditionGate>)>>,
    workers: Mutex<Option<JoinSet<bool>>>,
}

pub struct EditionGate {
    entered: Semaphore,
    release: Semaphore,
    retired: Semaphore,
}

#[allow(dead_code)]
impl EditionGate {
    pub async fn entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }

    pub fn release(&self) {
        self.release.add_permits(1);
    }

    pub fn has_retired(&self) -> bool {
        self.retired.available_permits() != 0
    }

    pub async fn retired(&self) {
        self.retired.acquire().await.unwrap().forget();
    }
}

struct HeldEdition(Arc<EditionGate>);

impl Drop for HeldEdition {
    fn drop(&mut self) {
        self.0.retired.add_permits(1);
    }
}

pub struct CatalogServer {
    pub url: String,
    requests: Arc<Requests>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let requests = Arc::new(Requests {
            workers: Mutex::new(Some(JoinSet::new())),
            ..Default::default()
        });
        let router = Router::new().fallback(respond).with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        Self {
            url,
            requests,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn enable_edition_listing(&self) {
        self.requests.listing_enabled.store(true, Ordering::SeqCst);
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        self.requests.observed.lock().unwrap().clone()
    }

    pub fn unexpected(&self) -> usize {
        self.requests.unexpected.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub fn hold(&self, operation: CatalogEditionOperation) -> Arc<EditionGate> {
        let gate = Arc::new(EditionGate {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            retired: Semaphore::new(0),
        });
        let mut held = self.requests.held.lock().unwrap();
        assert!(held.is_none(), "Only one next response may be gated");
        *held = Some((operation, gate.clone()));
        gate
    }

    pub async fn finish(&mut self) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut workers = self.requests.workers.lock().unwrap().take();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let completed = tokio::time::timeout_at(deadline, async {
            let mut joined = true;
            if let Some(workers) = workers.as_mut() {
                while let Some(result) = workers.join_next().await {
                    joined &= matches!(result, Ok(true));
                }
            }
            if let Some(task) = self.task.as_mut() {
                joined &= matches!(task.await, Ok(true));
            }
            joined
        })
        .await;
        match completed {
            Ok(joined) => {
                self.task.take();
                joined
            }
            Err(_) => {
                if let Some(task) = self.task.as_ref() {
                    task.abort();
                }
                if let Some(workers) = workers.as_mut() {
                    workers.shutdown().await;
                }
                if let Some(task) = self.task.take() {
                    let _ = task.await;
                }
                false
            }
        }
    }
}

impl Drop for CatalogServer {
    fn drop(&mut self) {
        if let Some(mut workers) = self.requests.workers.lock().unwrap().take() {
            workers.abort_all();
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn respond(State(requests): State<Arc<Requests>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, 4096).await else {
        return reject(&requests);
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        let Ok(value) = serde_json::from_slice(&bytes) else {
            return reject(&requests);
        };
        value
    };
    if parts.uri.query().is_some() {
        return reject(&requests);
    }
    let path = parts.uri.path().to_owned();
    {
        let mut observed = requests.observed.lock().unwrap();
        if observed.len() >= 32 {
            return reject(&requests);
        }
        observed.push(ObservedRequest {
            method: parts.method.clone(),
            path: path.clone(),
            body: body.clone(),
        });
    }
    match (parts.method.as_str(), path.as_str()) {
        ("GET", "/prefix/v1/manifest") if bytes.is_empty() => Json(Manifest {
            id: "org.hito.artist-editions-fixture".into(),
            name: "Owned artist editions".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .into_response(),
        ("GET", "/prefix/v1/catalog/capabilities") if bytes.is_empty() => {
            Json(CatalogCapabilities {
                catalog_version: CATALOG_VERSION,
                operations: vec![
                    CatalogOperation::ArtistProfile,
                    CatalogOperation::ArtistDiscography,
                ],
            })
            .into_response()
        }
        ("POST", "/prefix/v1/catalog/artists/profile") => {
            let Ok(request) = serde_json::from_value::<ArtistProfileRequest>(body) else {
                return reject(&requests);
            };
            if request.validate().is_err() || request.artist_id != ARTIST_ID {
                return reject(&requests);
            }
            Json(
                json!({"artist": {"id": ARTIST_ID, "name": "Owned performer"},
                "sort_name": null, "kind": null, "disambiguation": null, "revision": null}),
            )
            .into_response()
        }
        ("POST", "/prefix/v1/catalog/artists/discography") => {
            let Ok(request) = serde_json::from_value::<ArtistDiscographyRequest>(body) else {
                return reject(&requests);
            };
            if request.validate().is_err()
                || request.artist_id != ARTIST_ID
                || request.cursor.is_some()
                || request.snapshot.is_some()
                || request.limit != 1
            {
                return reject(&requests);
            }
            Json(expected_discography()).into_response()
        }
        ("GET", "/prefix/v1/catalog/editions/capabilities") if bytes.is_empty() => {
            let mut operations = vec![CatalogEditionOperation::EditionTrackList];
            if requests.listing_enabled.load(Ordering::SeqCst) {
                operations.push(CatalogEditionOperation::ReleaseGroupEditions);
            }
            Json(CatalogEditionCapabilities {
                edition_version: CATALOG_EDITION_VERSION,
                operations,
            })
            .into_response()
        }
        ("POST", "/prefix/v1/catalog/release-groups/editions") => {
            let Ok(request) = serde_json::from_value::<ReleaseGroupEditionsRequest>(body) else {
                return reject(&requests);
            };
            if !requests.listing_enabled.load(Ordering::SeqCst)
                || request.validate().is_err()
                || request.release_group_id != GROUP_ID
                || request.limit != 1
                || request.snapshot.is_some()
                || !matches!(request.cursor.as_deref(), None | Some(PROVIDER_CURSOR))
            {
                return reject(&requests);
            }
            if !hold_response(&requests, CatalogEditionOperation::ReleaseGroupEditions).await {
                return StatusCode::GATEWAY_TIMEOUT.into_response();
            }
            Json(expected_editions(request.cursor.is_some())).into_response()
        }
        ("POST", "/prefix/v1/catalog/editions/track-list") => {
            let Ok(request) = serde_json::from_value::<EditionTrackListRequest>(body) else {
                return reject(&requests);
            };
            if request.validate().is_err()
                || request.release_group_id != GROUP_ID
                || request.edition_id != BONUS_ID
            {
                return reject(&requests);
            }
            if !hold_response(&requests, CatalogEditionOperation::EditionTrackList).await {
                return StatusCode::GATEWAY_TIMEOUT.into_response();
            }
            Json(expected_track_list()).into_response()
        }
        _ => reject(&requests),
    }
}

async fn hold_response(requests: &Requests, operation: CatalogEditionOperation) -> bool {
    let gate = {
        let mut held = requests.held.lock().unwrap();
        if held
            .as_ref()
            .is_some_and(|(expected, _)| *expected == operation)
        {
            held.take().map(|(_, gate)| gate)
        } else {
            None
        }
    };
    let Some(gate) = gate else {
        return true;
    };
    let (done, completed) = oneshot::channel();
    {
        let mut workers = requests.workers.lock().unwrap();
        let Some(workers) = workers.as_mut() else {
            return false;
        };
        let held = HeldEdition(gate.clone());
        workers.spawn(async move {
            let _held = held;
            gate.entered.add_permits(1);
            let released =
                match tokio::time::timeout(Duration::from_secs(10), gate.release.acquire()).await {
                    Ok(Ok(permit)) => {
                        permit.forget();
                        true
                    }
                    _ => false,
                };
            let _ = done.send(released);
            released
        });
    }
    completed.await.unwrap_or(false)
}

fn reject(requests: &Requests) -> Response {
    requests.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::BAD_REQUEST.into_response()
}

fn expected_discography() -> ArtistDiscographyPage {
    serde_json::from_value(json!({
        "artist_id": ARTIST_ID, "snapshot": null, "total_count": 1, "next_cursor": null,
        "items": [{"id": GROUP_ID, "title": "Owned album", "artist_credit": null,
            "kind": "album", "secondary_types": [], "first_release_date": null, "revision": null}]
    }))
    .unwrap()
}

pub fn expected_editions(last: bool) -> ReleaseGroupEditionsPage {
    ReleaseGroupEditionsPage {
        release_group_id: GROUP_ID.into(),
        snapshot: None,
        items: vec![CatalogEditionSummary {
            id: if last { BONUS_ID } else { STANDARD_ID }.into(),
            title: "Owned album".into(),
            artist_credit: None,
            disambiguation: Some(
                if last {
                    "bonus disc"
                } else {
                    "standard edition"
                }
                .into(),
            ),
            release_date: None,
            country: None,
            revision: None,
        }],
        total_count: Some(2),
        next_cursor: (!last).then(|| PROVIDER_CURSOR.into()),
    }
}

pub fn expected_track_list() -> CatalogEditionTrackList {
    serde_json::from_value(json!({
        "id": BONUS_ID, "release_group_id": GROUP_ID, "title": "Owned album",
        "artist_credit": null, "revision": null,
        "media": [
            {"position": 1, "title": "Main disc", "format": "CD", "tracks": [
                {"id": "occurrence:owned/disc-1/main?number=A1", "position": 1, "number": "A1",
                    "kind": "audio", "title": "Owned main track", "artist_credit": null,
                    "duration_ms": 120000, "recording_id": RECORDING_ID},
                {"id": "occurrence:owned/disc-1/second?number=A2", "position": 2, "number": "A2",
                    "kind": "audio", "title": "Owned second track", "artist_credit": null,
                    "duration_ms": null, "recording_id": null}
            ]},
            {"position": 2, "title": "Bonus disc", "format": null, "tracks": [
                {"id": "occurrence:owned/disc-2/repeat?number=1", "position": 1, "number": "1",
                    "kind": "audio", "title": "Owned repeated occurrence", "artist_credit": null,
                    "duration_ms": 120000, "recording_id": RECORDING_ID},
                {"id": "occurrence:owned/disc-2/bonus?number=Bonus", "position": 2, "number": "Bonus",
                    "kind": "audio", "title": "Owned bonus track", "artist_credit": null,
                    "duration_ms": null, "recording_id": null}
            ]}
        ]
    })).unwrap()
}
