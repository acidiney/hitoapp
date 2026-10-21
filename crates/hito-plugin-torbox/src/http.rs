use crate::{
    Config, InitError, audio,
    catalog::Catalog,
    policy::{Error, Policy},
    preparation::{self, Operations},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    middleware::from_fn_with_state,
    routing::{get, post},
};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    Capability, ContentPolicy, Manifest, PROTOCOL_VERSION, PreparationRequest,
    PreparationStatusRequest, PreparationStatusResponse, PreparationSubmitResponse,
    ProviderRecording, ProviderSource, RecordingRequest, SearchPage, SearchRequest, SourcePage,
    SourcePersistence,
};
use std::sync::Arc;

pub(crate) struct HttpState {
    pub(crate) catalog: Catalog,
    pub(crate) client: TorBoxClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) operations: Arc<Operations>,
}

pub(crate) fn router(config: Config) -> Result<Router, InitError> {
    let policy = Arc::new(Policy::new(config.service_bearer)?);
    let catalog = Catalog::new(config.recordings)?;
    let operations = Operations::new(catalog.len());
    let state = Arc::new(HttpState {
        catalog,
        client: config.client,
        media_origins: config.media_origins,
        operations,
    });
    Ok(Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/sources", post(sources))
        .route("/v1/sources/prepare", post(prepare))
        .route("/v1/sources/preparation/status", post(status))
        .route("/v1/audio", get(audio::response))
        .layer(DefaultBodyLimit::max(4096))
        .layer(from_fn_with_state(policy, crate::policy::observe))
        .with_state(state))
}

async fn manifest() -> Json<Manifest> {
    Json(Manifest {
        id: "org.hito.torbox".into(),
        name: "TorBox curated WAV".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [
            Capability::Search,
            Capability::Sources,
            Capability::SourcePreparation,
        ]
        .into(),
        authorization: None,
        content_policy: Some(ContentPolicy {
            source_persistence: SourcePersistence::PlaybackOnly,
        }),
    })
}

async fn search(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    state.catalog.search(request).map(Json)
}

async fn detail(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<ProviderRecording>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let index = state.catalog.recording(&request.recording_id)?;
    Ok(Json(state.catalog.entry(index).recording.clone()))
}

async fn sources(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<SourcePage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let index = state.catalog.recording(&request.recording_id)?;
    let ready = preparation::ready(&state, index).await?;
    let entry = state.catalog.entry(index);
    Ok(Json(SourcePage {
        items: vec![ProviderSource {
            id: entry.source_id.clone(),
            label: entry.label.clone(),
            ready,
        }],
    }))
}

async fn prepare(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<PreparationRequest>, JsonRejection>,
) -> Result<Json<PreparationSubmitResponse>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let index = state
        .catalog
        .target(&request.recording_id, &request.source_id)?;
    let step = state.operations.submit(request.operation_id, index)?;
    let (handle, status) = preparation::settle(&state, index, step).await?;
    Ok(Json(PreparationSubmitResponse {
        operation_id: request.operation_id,
        recording_id: request.recording_id,
        source_id: request.source_id,
        submission: hito_plugin_sdk::PreparationSubmission::Accepted { handle, status },
    }))
}

async fn status(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<PreparationStatusRequest>, JsonRejection>,
) -> Result<Json<PreparationStatusResponse>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let index = state
        .catalog
        .target(&request.recording_id, &request.source_id)?;
    let step = state
        .operations
        .poll(request.operation_id, index, &request.handle)?;
    let (handle, status) = preparation::settle(&state, index, step).await?;
    Ok(Json(PreparationStatusResponse {
        operation_id: request.operation_id,
        recording_id: request.recording_id,
        source_id: request.source_id,
        handle,
        status,
    }))
}
