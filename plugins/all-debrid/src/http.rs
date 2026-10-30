use crate::{Catalog, Config, InitError, audio, preparation, receipts};
use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    middleware::from_fn_with_state,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_debrid_all_debrid::{AllDebridClient, MagnetPhase, MediaAllowlist};
use hito_plugin_sdk::{
    Capability, ContentPolicy, Manifest, PROTOCOL_VERSION, PreparationRequest,
    PreparationStatusRequest, PreparationStatusResponse, PreparationSubmitResponse,
    ProviderRecording, ProviderSource, RecordingRequest, SearchPage, SearchRequest, SourcePage,
    SourcePersistence,
    server::{RequestPolicy, ServiceAuth, observe_requests, with_manifest},
};
use std::sync::Arc;
use zeroize::Zeroizing;

pub(crate) struct StateData {
    pub(crate) catalog: Catalog,
    pub(crate) client: AllDebridClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) receipts: receipts::ReceiptKey,
}

pub fn router(config: Config) -> Result<Router, InitError> {
    let bearer = Zeroizing::new(config.service_bearer);
    let auth = ServiceAuth::new(&bearer).ok_or(InitError::InvalidServiceCredential)?;
    let policy = Arc::new(RequestPolicy::new(
        hito_plugin_sdk::request_telemetry!("all_debrid"),
        Some(auth),
    ));
    let state = Arc::new(StateData {
        receipts: receipts::ReceiptKey::new(config.receipt_key)?,
        catalog: Catalog::new(config.recordings)?,
        client: config.client,
        media_origins: config.media_origins,
    });
    let routes = Router::new()
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/sources", post(sources))
        .route("/v1/sources/prepare", post(prepare))
        .route("/v1/sources/preparation/status", post(status))
        .route("/v1/audio", get(audio::response));
    Ok(with_manifest(routes, manifest(), 4096)
        .layer(from_fn_with_state(policy, observe_requests))
        .with_state(state))
}

fn manifest() -> Manifest {
    Manifest {
        id: "org.hito.all-debrid".into(),
        name: "AllDebrid curated WAV".into(),
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
    State(state): State<Arc<StateData>>,
    payload: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    state.catalog.search(request).map(Json)
}

async fn detail(
    State(state): State<Arc<StateData>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<ProviderRecording>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    Ok(Json(
        state
            .catalog
            .recording(&request.recording_id)?
            .configured
            .recording
            .clone(),
    ))
}

async fn sources(
    State(state): State<Arc<StateData>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<SourcePage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let entry = state.catalog.recording(&request.recording_id)?;
    let ready = if preparation::has_candidate(&entry)? {
        match state.client.observe(&entry.configured.binding).await {
            Ok(observed) if observed.phase == MagnetPhase::ProviderCompleted => {
                preparation::has_candidate(&entry)?
            }
            Ok(_) => false,
            Err(hito_debrid_all_debrid::Error::SourceChanged) => {
                preparation::invalidate(&entry)?;
                false
            }
            Err(_) => return Err(Error::Unavailable),
        }
    } else {
        false
    };
    Ok(Json(SourcePage {
        items: vec![ProviderSource {
            id: entry.configured.source_id.clone(),
            label: entry.configured.label.clone(),
            ready,
        }],
    }))
}

async fn prepare(
    State(state): State<Arc<StateData>>,
    payload: Result<Json<PreparationRequest>, JsonRejection>,
) -> Result<Json<PreparationSubmitResponse>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let entry = state.catalog.recording(&request.recording_id)?;
    if entry.configured.source_id != request.source_id {
        return Err(Error::NotFound);
    }
    preparation::submit(&state, entry, &request).await.map(Json)
}

async fn status(
    State(state): State<Arc<StateData>>,
    payload: Result<Json<PreparationStatusRequest>, JsonRejection>,
) -> Result<Json<PreparationStatusResponse>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let entry = state.catalog.recording(&request.recording_id)?;
    if entry.configured.source_id != request.source_id {
        return Err(Error::NotFound);
    }
    preparation::status(&state, entry, &request).await.map(Json)
}

pub(crate) enum Error {
    InvalidRequest,
    NotFound,
    Conflict,
    Unavailable,
    Busy,
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The request is invalid"),
            Self::NotFound => (StatusCode::NOT_FOUND, "The requested source is unavailable"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "The operation belongs to another source",
            ),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "The source is unavailable"),
            Self::Busy => (StatusCode::TOO_MANY_REQUESTS, "The service is busy"),
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}
