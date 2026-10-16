use crate::{catalog::Catalog, extract::JsonBody};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, Manifest, ProviderSource, RecordingRequest, SearchPage, SearchRequest, SourcePage,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Semaphore;
use tracing::Instrument;

pub(crate) struct HttpState {
    pub catalog: Arc<Catalog>,
    pub audio_slots: Arc<Semaphore>,
    pub file_slots: Arc<Semaphore>,
    request_slots: Arc<Semaphore>,
}

pub(crate) fn router(catalog: Arc<Catalog>) -> Router {
    let state = Arc::new(HttpState {
        catalog,
        audio_slots: Arc::new(Semaphore::new(8)),
        file_slots: Arc::new(Semaphore::new(4)),
        request_slots: Arc::new(Semaphore::new(16)),
    });
    Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/sources", post(sources))
        .route("/v1/audio", get(crate::audio::audio))
        .layer(DefaultBodyLimit::max(4096))
        .layer(axum::middleware::from_fn_with_state(state.clone(), observe))
        .with_state(state)
}
async fn manifest() -> Json<Manifest> {
    Json(Manifest {
        id: "org.hito.music-folder".into(),
        name: "Hito music folder".into(),
        protocol_version: 1,
        capabilities: [Capability::Search, Capability::Sources].into(),
    })
}
async fn search(
    State(state): State<Arc<HttpState>>,
    JsonBody(request): JsonBody<SearchRequest>,
) -> Result<Json<SearchPage>, StatusCode> {
    crate::search::page(&state.catalog, request).map(Json)
}
async fn detail(
    State(state): State<Arc<HttpState>>,
    JsonBody(request): JsonBody<RecordingRequest>,
) -> Result<Response, StatusCode> {
    let catalog = &state.catalog;
    request.validate().map_err(|_| StatusCode::BAD_REQUEST)?;
    let recording = catalog
        .retained
        .get(&request.recording_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(recording).into_response())
}
async fn sources(
    State(state): State<Arc<HttpState>>,
    JsonBody(request): JsonBody<RecordingRequest>,
) -> Result<Json<SourcePage>, StatusCode> {
    let catalog = &state.catalog;
    request.validate().map_err(|_| StatusCode::BAD_REQUEST)?;
    if !catalog.retained.contains_key(&request.recording_id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let ids = catalog
        .recording_sources
        .get(&request.recording_id)
        .cloned()
        .unwrap_or_default();
    if ids.is_empty() {
        return Ok(Json(SourcePage { items: Vec::new() }));
    }
    let owned = state.catalog.clone();
    let permit = state
        .file_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let page = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            let ready = owned.open_source(&id, deadline)?.is_some();
            items.push(ProviderSource {
                id,
                label: "Original folder audio".into(),
                ready,
            });
        }
        Ok::<_, std::io::Error>(SourcePage { items })
    })
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(page))
}

async fn observe(
    State(state): State<Arc<HttpState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let request_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/search" => "search",
        "/v1/recordings/detail" => "detail",
        "/v1/sources" => "sources",
        "/v1/audio" => "audio",
        _ => "unknown",
    };
    let span = tracing::info_span!("folder_request", request_id, operation);
    async move {
        let started = std::time::Instant::now();
        let Ok(_permit) = state.request_slots.clone().try_acquire_owned() else {
            tracing::debug!(event = "folder_request_rejected", outcome = "capacity");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        if request
            .uri()
            .query()
            .is_some_and(|query| query.len() > 4096)
        {
            return StatusCode::URI_TOO_LONG.into_response();
        }
        let response =
            match tokio::time::timeout(std::time::Duration::from_secs(10), next.run(request)).await
            {
                Ok(response) => response,
                Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
            };
        tracing::debug!(
            event = "folder_request_complete",
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        response
    }
    .instrument(span)
    .await
}
