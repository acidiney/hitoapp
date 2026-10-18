use axum::{
    Json, Router,
    body::Body,
    extract::{OriginalUri, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_core::{Actor, InstallPlugin, OfferPluginService, PluginInstallation, Services};
use hito_plugin_sdk::{
    ArtworkEntity, ArtworkRequest, PlaylistDetailRequest, PlaylistItemsRequest, SearchRequest,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    io::AsyncWriteExt,
    sync::{Mutex, Semaphore},
};

pub const PROVIDER_RECORDING_ID: &str = "recording/retained-cover";
pub const COVER: &[u8] = include_bytes!("cover-a.png");
const PLAYLIST_ID: &str = "playlist/retained-cover";
const SNAPSHOT: &str = "owned-snapshot";

struct Fixture {
    requests: Mutex<Vec<Value>>,
    entered: Semaphore,
    release: Semaphore,
    bytes: Vec<u8>,
    content_type: &'static str,
    search_enabled: bool,
}

pub struct ArtworkServer {
    url: String,
    state: Arc<Fixture>,
    task: tokio::task::JoinHandle<()>,
}

impl ArtworkServer {
    #[allow(dead_code)]
    pub async fn start() -> Self {
        Self::start_with(COVER.to_vec(), "image/png").await
    }

    pub async fn start_with(bytes: Vec<u8>, content_type: &'static str) -> Self {
        Self::configured(bytes, content_type, false).await
    }

    #[allow(dead_code)]
    pub async fn start_with_search() -> Self {
        Self::configured(COVER.to_vec(), "image/png", true).await
    }

    async fn configured(bytes: Vec<u8>, content_type: &'static str, search_enabled: bool) -> Self {
        let state = Arc::new(Fixture {
            requests: Mutex::new(Vec::new()),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            bytes,
            content_type,
            search_enabled,
        });
        let router = Router::new()
            .route("/prefix/v1/manifest", get(manifest))
            .route("/prefix/v1/playlists/detail", post(playlist_detail))
            .route("/prefix/v1/playlists/items", post(playlist_items))
            .route("/prefix/v1/artwork", post(artwork))
            .route("/prefix/v1/search", post(search))
            .fallback(unexpected)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, state, task }
    }

    pub async fn connect(&self, hito: &Services, actor: &Actor) -> PluginInstallation {
        let service = hito
            .plugins
            .offer_service(
                actor,
                OfferPluginService {
                    name: "Owned artwork fixture".into(),
                    base_url: self.url.clone(),
                },
            )
            .await
            .unwrap();
        hito.plugins
            .install(
                actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap()
    }

    pub async fn requests(&self) -> Vec<Value> {
        self.state.requests.lock().await.clone()
    }

    pub async fn image_entered(&self) {
        self.state.entered.acquire().await.unwrap().forget();
    }

    pub fn release_image(&self) {
        self.state.release.add_permits(1);
    }
}

impl Drop for ArtworkServer {
    fn drop(&mut self) {
        self.state.release.close();
        self.task.abort();
    }
}

async fn record(state: &Fixture, method: &str, path: &str, body: Value) {
    state.requests.lock().await.push(json!({
        "method": method, "path": path, "body": body
    }));
}

async fn manifest(State(state): State<Arc<Fixture>>) -> Json<Value> {
    record(&state, "GET", "v1/manifest", Value::Null).await;
    let capabilities = if state.search_enabled {
        json!(["playlists", "artwork", "search"])
    } else {
        json!(["playlists", "artwork"])
    };
    Json(json!({
        "id": "org.hito.owned-artwork", "name": "Owned artwork fixture",
        "protocol_version": 1, "capabilities": capabilities
    }))
}

async fn search(State(state): State<Arc<Fixture>>, Json(request): Json<SearchRequest>) -> Response {
    request.validate().unwrap();
    record(
        &state,
        "POST",
        "v1/search",
        serde_json::to_value(request).unwrap(),
    )
    .await;
    if !state.search_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(json!({"items": [], "next_cursor": null})).into_response()
}

async fn playlist_detail(
    State(state): State<Arc<Fixture>>,
    Json(request): Json<PlaylistDetailRequest>,
) -> Json<Value> {
    request.validate().unwrap();
    record(
        &state,
        "POST",
        "v1/playlists/detail",
        serde_json::to_value(request).unwrap(),
    )
    .await;
    Json(json!({
        "id": PLAYLIST_ID, "name": "Owned selection", "snapshot": SNAPSHOT,
        "total_count": 2, "attribution": {"label": "Owned artwork fixture"}
    }))
}

async fn playlist_items(
    State(state): State<Arc<Fixture>>,
    Json(request): Json<PlaylistItemsRequest>,
) -> Json<Value> {
    request.validate().unwrap();
    assert_eq!(request.playlist_id, PLAYLIST_ID);
    assert_eq!(request.snapshot, SNAPSHOT);
    assert!(request.cursor.is_none());
    record(
        &state,
        "POST",
        "v1/playlists/items",
        serde_json::to_value(request).unwrap(),
    )
    .await;
    let item = json!({"kind": "recording", "recording": {
        "id": PROVIDER_RECORDING_ID, "title": "Provider title", "artist": "Provider artist",
        "duration_ms": 123_000, "revision": "owned-recording-revision"
    }});
    Json(json!({
        "playlist_id": PLAYLIST_ID, "snapshot": SNAPSHOT, "next_cursor": null,
        "items": [
            {"occurrence_id": "first", "item": item},
            {"occurrence_id": "again", "item": item}
        ]
    }))
}

async fn artwork(
    State(state): State<Arc<Fixture>>,
    Json(request): Json<ArtworkRequest>,
) -> Response {
    request.validate().unwrap();
    assert_eq!(request.entity, ArtworkEntity::Recording);
    assert_eq!(request.provider_id, PROVIDER_RECORDING_ID);
    record(
        &state,
        "POST",
        "v1/artwork",
        serde_json::to_value(request).unwrap(),
    )
    .await;
    let (mut writer, reader) = tokio::io::duplex(1024);
    let length = state.bytes.len().to_string();
    let content_type = state.content_type;
    tokio::spawn(async move {
        let prefix = state.bytes.len().min(8);
        if writer.write_all(&state.bytes[..prefix]).await.is_err() {
            return;
        }
        state.entered.add_permits(1);
        let Ok(permit) = state.release.acquire().await else {
            return;
        };
        permit.forget();
        let _ = writer.write_all(&state.bytes[prefix..]).await;
    });
    (
        [
            ("content-type", content_type),
            ("content-length", length.as_str()),
        ],
        Body::from_stream(tokio_util::io::ReaderStream::new(reader)),
    )
        .into_response()
}

async fn unexpected(
    State(state): State<Arc<Fixture>>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    body: String,
) -> StatusCode {
    record(
        &state,
        method.as_str(),
        uri.path().trim_start_matches("/prefix/"),
        serde_json::from_str(&body).unwrap_or(Value::Null),
    )
    .await;
    StatusCode::NOT_FOUND
}
