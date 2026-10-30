use crate::{
    audio,
    catalog::Catalog,
    journal::{Handle, Target},
    policy::Error,
    preparation,
};
use axum::{
    Json, Router,
    extract::{Request, State, rejection::JsonRejection},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    Capability, ContentPolicy, Manifest, PROTOCOL_VERSION, PreparationRequest,
    PreparationStatusRequest, PreparationStatusResponse, PreparationSubmitResponse,
    ProviderRecording, ProviderSource, RecordingRequest, SearchPage, SearchRequest, SourcePage,
    SourcePersistence,
    server::{RequestPolicy, observe_requests, with_manifest},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) struct HttpState {
    pub(crate) catalog: Catalog,
    pub(crate) client: TorBoxClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) journal: Handle,
}

pub(crate) fn router(
    policy: RequestPolicy,
    catalog: Catalog,
    client: TorBoxClient,
    media_origins: MediaAllowlist,
    gate: Arc<AtomicBool>,
    journal: Handle,
) -> Router {
    let policy = Arc::new(policy);
    let state = Arc::new(HttpState {
        catalog,
        client,
        media_origins,
        journal,
    });
    let routes = Router::new()
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/sources", post(sources))
        .route("/v1/sources/prepare", post(prepare))
        .route("/v1/sources/preparation/status", post(status))
        .route("/v1/audio", get(audio::response));
    with_manifest(routes, manifest(), 4096)
        .layer(from_fn_with_state(gate, admitted))
        .layer(from_fn_with_state(policy, observe_requests))
        .with_state(state)
}

async fn admitted(State(gate): State<Arc<AtomicBool>>, request: Request, next: Next) -> Response {
    if !gate.load(Ordering::Acquire) {
        return Error::Unavailable.into_response();
    }
    next.run(request).await
}

fn manifest() -> Manifest {
    Manifest {
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
    }
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
    let step = state
        .journal
        .submit(
            request.operation_id,
            Target::from(state.catalog.entry(index)),
        )
        .await?;
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
        .journal
        .poll(
            request.operation_id,
            Target::from(state.catalog.entry(index)),
            &request.handle,
        )
        .await?;
    let (handle, status) = preparation::settle(&state, index, step).await?;
    Ok(Json(PreparationStatusResponse {
        operation_id: request.operation_id,
        recording_id: request.recording_id,
        source_id: request.source_id,
        handle,
        status,
    }))
}
