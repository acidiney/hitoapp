use crate::{Actor, InstallPlugin, OfferPluginService, PluginInstallation, Services};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest, PlaylistDetailRequest, PlaylistItemsRequest};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;

pub const RECORDING_ID: &str = "recording/owned-artwork";
pub const RELEASE_ID: &str = "release/verified-artwork";
pub const RECORDING_COVER: &[u8] = super::covers::COVER_A;
pub const RELEASE_COVER: &[u8] = super::covers::COVER_B;
pub const PLAYLIST_COVER: &[u8] = super::covers::PLAYLIST_COVER;
pub const PLAYLIST_ID: &str = "playlist/owned-artwork";
const SNAPSHOT: &str = "owned-artwork-snapshot";

pub struct ArtworkServer {
    pub url: String,
    requests: Arc<Mutex<Vec<ArtworkRequest>>>,
    artwork_enabled: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl ArtworkServer {
    pub async fn start() -> Self {
        Self::configured(true).await
    }

    pub async fn start_without_artwork() -> Self {
        Self::configured(false).await
    }

    async fn configured(artwork_enabled: bool) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let artwork_enabled = Arc::new(AtomicBool::new(artwork_enabled));
        let manifest_artwork = artwork_enabled.clone();
        let app = Router::new()
            .route(
                "/v1/manifest",
                get(move || manifest(manifest_artwork.clone())),
            )
            .route("/v1/playlists/detail", post(playlist_detail))
            .route("/v1/playlists/items", post(playlist_items))
            .route("/v1/artwork", post(artwork))
            .with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            requests,
            artwork_enabled,
            task,
        }
    }

    pub fn enable_artwork(&self) {
        self.artwork_enabled.store(true, Ordering::SeqCst);
    }

    pub async fn connect(&self, hito: &Services, actor: &Actor) -> PluginInstallation {
        let service = hito
            .plugins
            .offer_service(
                actor,
                OfferPluginService {
                    name: "Owned collection artwork".into(),
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

    pub async fn requests(&self) -> Vec<ArtworkRequest> {
        self.requests.lock().await.clone()
    }
}

impl Drop for ArtworkServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn manifest(artwork_enabled: Arc<AtomicBool>) -> Json<Value> {
    let capabilities = if artwork_enabled.load(Ordering::SeqCst) {
        json!(["playlists", "artwork"])
    } else {
        json!(["playlists"])
    };
    Json(json!({
        "id": "org.hito.owned-collection-artwork", "name": "Owned collection artwork",
        "protocol_version": 1, "capabilities": capabilities
    }))
}

async fn playlist_detail(Json(request): Json<PlaylistDetailRequest>) -> Json<Value> {
    request.validate().unwrap();
    Json(json!({
        "id": PLAYLIST_ID, "name": "Owned artwork selection", "snapshot": SNAPSHOT,
        "total_count": 2, "attribution": {"label": "Owned collection artwork"}
    }))
}

async fn playlist_items(Json(request): Json<PlaylistItemsRequest>) -> Json<Value> {
    request.validate().unwrap();
    assert_eq!(request.playlist_id, PLAYLIST_ID);
    assert_eq!(request.snapshot, SNAPSHOT);
    assert!(request.cursor.is_none());
    let item = json!({"kind": "recording", "recording": {
        "id": RECORDING_ID, "title": "Observed recording", "artist": "Observed performer",
        "duration_ms": 123_000, "revision": "owned-recording-revision",
        "release": {"id": RELEASE_ID, "title": "Observed album"}
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
    State(requests): State<Arc<Mutex<Vec<ArtworkRequest>>>>,
    Json(request): Json<ArtworkRequest>,
) -> Response {
    request.validate().unwrap();
    let bytes = match request.entity {
        ArtworkEntity::Recording if request.provider_id == RECORDING_ID => RECORDING_COVER,
        ArtworkEntity::Release if request.provider_id == RELEASE_ID => RELEASE_COVER,
        ArtworkEntity::Playlist if request.provider_id == PLAYLIST_ID => PLAYLIST_COVER,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    requests.lock().await.push(request);
    ([("content-type", "image/png")], bytes).into_response()
}
