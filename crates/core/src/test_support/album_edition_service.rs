use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    ArtistProfileRequest, CATALOG_EDITION_VERSION, CATALOG_VERSION, Capability,
    CatalogCapabilities, CatalogEditionCapabilities, CatalogEditionOperation,
    EditionTrackListRequest, Manifest, PROTOCOL_VERSION, RecordingRequest,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{sync::oneshot, task::JoinHandle};

pub const ARTIST_ID: &str = "artist:album/owned?credit=lead";
pub const CREDIT: &str = "Owned album performer";
pub const GROUP_ID: &str = "group:album/owned?kind=album";
pub const EDITION_ID: &str = "edition:album/deluxe?pressing=二";
pub const STANDARD_ID: &str = "edition:album/standard?pressing=一";
pub const RELEASE_TITLE: &str = "Owned album";
pub const FIRST: &str = "recording:album/first";
pub const SECOND: &str = "recording:album/second";
pub const EXTRA: &str = "recording:album/library-only";
pub const BONUS: &str = "recording:album/bonus";
pub const FEATURED: &str = "recording:album/featured-guest";
pub const FEATURED_CREDIT: &str = "Featured guest performer";
pub const TWIN: &str = "recording:album/same-title-other-recording";
pub const FIRST_TITLE: &str = "Shared title";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedRequest {
    pub method: Method,
    pub path: String,
    pub body: Value,
}

struct State_ {
    observed: Mutex<Vec<ObservedRequest>>,
    unexpected: AtomicUsize,
    outage: AtomicBool,
    editions: AtomicBool,
}

pub struct AlbumProvider {
    pub url: String,
    state: Arc<State_>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl AlbumProvider {
    pub async fn start() -> Self {
        let state = Arc::new(State_ {
            observed: Mutex::default(),
            unexpected: AtomicUsize::new(0),
            outage: AtomicBool::new(false),
            editions: AtomicBool::new(true),
        });
        let router = Router::new().fallback(respond).with_state(state.clone());
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
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn set_outage(&self, outage: bool) {
        self.state.outage.store(outage, Ordering::SeqCst);
    }

    pub fn set_editions(&self, supported: bool) {
        self.state.editions.store(supported, Ordering::SeqCst);
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        self.state.observed.lock().unwrap().clone()
    }

    pub fn requests_to(&self, path: &str) -> Vec<ObservedRequest> {
        self.requests()
            .into_iter()
            .filter(|request| request.path == format!("/prefix/{path}"))
            .collect()
    }

    pub fn unexpected(&self) -> usize {
        self.state.unexpected.load(Ordering::SeqCst)
    }

    pub async fn finish(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        match self.task.take() {
            Some(task) => matches!(task.await, Ok(true)),
            None => true,
        }
    }
}

impl Drop for AlbumProvider {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub fn detail(id: &str) -> Option<Value> {
    let (title, disc, track, artist) = match id {
        FIRST => (FIRST_TITLE, 1, 2, CREDIT),
        SECOND => ("Second provider title", 1, 3, CREDIT),
        EXTRA => ("Library only track", 1, 9, CREDIT),
        FEATURED => ("Featured provider title", 1, 1, FEATURED_CREDIT),
        BONUS => ("Bonus provider title", 2, 2, CREDIT),
        _ => return None,
    };
    Some(json!({
        "id": id, "title": title, "artist": artist, "duration_ms": 1000,
        "release": {"id": EDITION_ID, "title": RELEASE_TITLE, "artist": CREDIT},
        "disc_number": disc, "track_number": track, "revision": "album-r1"
    }))
}

pub fn track_list(edition: &str) -> Option<Value> {
    match edition {
        EDITION_ID => Some(deluxe()),
        STANDARD_ID => Some(json!({
            "id": STANDARD_ID, "release_group_id": GROUP_ID, "title": RELEASE_TITLE,
            "artist_credit": null, "revision": null,
            "media": [{"position": 1, "title": null, "format": "CD", "tracks": [
                {"id": "occurrence:standard/1/1", "position": 1, "number": "1", "kind": "audio",
                    "title": "Standard first", "artist_credit": null, "duration_ms": 1000,
                    "recording_id": FIRST}
            ]}]
        })),
        _ => None,
    }
}

fn deluxe() -> Value {
    let occurrence = |id: &str, position: u32, kind: &str, title: &str, recording: Option<&str>| {
        json!({
            "id": id, "position": position,
            "number": if position == 0 { Value::Null } else { json!(position.to_string()) },
            "kind": kind, "title": title, "artist_credit": null,
            "duration_ms": 1000, "recording_id": recording
        })
    };
    json!({
        "id": EDITION_ID, "release_group_id": GROUP_ID, "title": RELEASE_TITLE,
        "artist_credit": null, "revision": null,
        "media": [
            {"position": 1, "title": "Main disc", "format": "CD", "tracks": [
                occurrence("occurrence:album/1/0", 0, "pregap", "Hidden intro", None),
                occurrence("occurrence:album/1/1", 1, "audio", "First catalog title", Some(FIRST)),
                occurrence("occurrence:album/1/2", 2, "audio", "Second catalog title", Some(SECOND)),
                occurrence("occurrence:album/1/3", 3, "audio", FIRST_TITLE, Some(TWIN))
            ]},
            {"position": 2, "title": "Bonus disc", "format": null, "tracks": [
                occurrence("occurrence:album/2/1", 1, "audio", "First catalog title", Some(FIRST)),
                occurrence("occurrence:album/2/2", 2, "audio", "Bonus catalog title", Some(BONUS)),
                occurrence("occurrence:album/2/3", 3, "audio", "Unidentified bonus", None)
            ]}
        ]
    })
}

async fn respond(State(state): State<Arc<State_>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, 4096).await else {
        return reject(&state);
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        let Ok(value) = serde_json::from_slice(&bytes) else {
            return reject(&state);
        };
        value
    };
    let path = parts.uri.path().to_owned();
    {
        let mut observed = state.observed.lock().unwrap();
        if observed.len() >= 64 || parts.uri.query().is_some() {
            return reject(&state);
        }
        observed.push(ObservedRequest {
            method: parts.method.clone(),
            path: path.clone(),
            body: body.clone(),
        });
    }
    match (parts.method.as_str(), path.as_str()) {
        ("GET", "/prefix/v1/manifest") => Json(Manifest {
            id: "org.hito.album-edition-fixture".into(),
            name: "Owned album editions".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog, Capability::Search, Capability::Metadata].into(),
            authorization: None,
            content_policy: None,
        })
        .into_response(),
        ("GET", "/prefix/v1/catalog/capabilities") => Json(CatalogCapabilities {
            catalog_version: CATALOG_VERSION,
            operations: vec![
                hito_plugin_sdk::CatalogOperation::ArtistProfile,
                hito_plugin_sdk::CatalogOperation::ArtistDiscography,
            ],
        })
        .into_response(),
        ("GET", "/prefix/v1/catalog/editions/capabilities") => {
            let operations = if state.editions.load(Ordering::SeqCst) {
                vec![
                    CatalogEditionOperation::ReleaseGroupEditions,
                    CatalogEditionOperation::EditionTrackList,
                ]
            } else {
                vec![CatalogEditionOperation::ReleaseGroupEditions]
            };
            Json(CatalogEditionCapabilities {
                edition_version: CATALOG_EDITION_VERSION,
                operations,
            })
            .into_response()
        }
        ("POST", "/prefix/v1/catalog/artists/profile") => {
            match serde_json::from_value::<ArtistProfileRequest>(body) {
                Ok(request) if request.artist_id == ARTIST_ID => Json(json!({
                    "artist": {"id": ARTIST_ID, "name": CREDIT},
                    "sort_name": null, "kind": null, "disambiguation": null, "revision": null
                }))
                .into_response(),
                _ => reject(&state),
            }
        }
        ("POST", "/prefix/v1/catalog/artists/discography") => Json(json!({
            "artist_id": ARTIST_ID, "snapshot": null, "total_count": 1, "next_cursor": null,
            "items": [{"id": GROUP_ID, "title": RELEASE_TITLE, "artist_credit": null,
                "kind": "album", "secondary_types": [], "first_release_date": null, "revision": null}]
        }))
        .into_response(),
        ("POST", "/prefix/v1/catalog/release-groups/editions") => Json(json!({
            "release_group_id": GROUP_ID, "snapshot": null, "total_count": 1, "next_cursor": null,
            "items": [{"id": EDITION_ID, "title": RELEASE_TITLE, "artist_credit": null,
                "disambiguation": "deluxe", "release_date": null, "country": null, "revision": null}]
        }))
        .into_response(),
        ("POST", "/prefix/v1/catalog/editions/track-list") => {
            let Ok(request) = serde_json::from_value::<EditionTrackListRequest>(body) else {
                return reject(&state);
            };
            let Some(list) = track_list(&request.edition_id) else {
                return reject(&state);
            };
            if request.release_group_id != GROUP_ID {
                return reject(&state);
            }
            if state.outage.load(Ordering::SeqCst) {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            Json(list).into_response()
        }
        ("POST", "/prefix/v1/recordings/detail") => {
            match serde_json::from_value::<RecordingRequest>(body)
                .ok()
                .and_then(|request| detail(&request.recording_id))
            {
                Some(value) => Json(value).into_response(),
                None => reject(&state),
            }
        }
        _ => reject(&state),
    }
}

fn reject(state: &State_) -> Response {
    state.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::BAD_REQUEST.into_response()
}
