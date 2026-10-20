use crate::{Catalog, Config, InitError, audio, preparation, receipts};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_debrid_all_debrid::{AllDebridClient, MagnetPhase, MediaAllowlist};
use hito_plugin_sdk::{
    Capability, ContentPolicy, Manifest, PROTOCOL_VERSION, PreparationRequest,
    PreparationStatusRequest, PreparationStatusResponse, PreparationSubmitResponse,
    ProviderRecording, ProviderSource, RecordingRequest, SearchPage, SearchRequest, SourcePage,
    SourcePersistence,
};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use tracing::Instrument;
use zeroize::Zeroizing;

pub(crate) struct StateData {
    pub(crate) catalog: Catalog,
    pub(crate) client: AllDebridClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) receipts: receipts::ReceiptKey,
    authentication: [u8; 32],
    slots: Arc<Semaphore>,
}

pub fn router(config: Config) -> Result<Router, InitError> {
    let bearer = Zeroizing::new(config.service_bearer);
    if !valid_token(&bearer) {
        return Err(InitError::InvalidServiceCredential);
    }
    let state = Arc::new(StateData {
        authentication: Sha256::digest(bearer.as_bytes()).into(),
        receipts: receipts::ReceiptKey::new(config.receipt_key)?,
        catalog: Catalog::new(config.recordings)?,
        client: config.client,
        media_origins: config.media_origins,
        slots: Arc::new(Semaphore::new(16)),
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
        .layer(from_fn_with_state(state.clone(), observe))
        .with_state(state))
}

async fn manifest() -> Json<Manifest> {
    Json(Manifest {
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
    })
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

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 4096
        && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn authenticate(state: &StateData, headers: &HeaderMap) -> Result<(), Error> {
    let mut values = headers.get_all("authorization").iter();
    let supplied = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| valid_token(value))
        .ok_or(Error::Unauthorized)?;
    if values.next().is_some() {
        return Err(Error::Unauthorized);
    }
    let digest: [u8; 32] = Sha256::digest(supplied.as_bytes()).into();
    if !bool::from(digest.ct_eq(&state.authentication)) {
        return Err(Error::Unauthorized);
    }
    let mut protocol = headers.get_all("x-hito-plugin-protocol").iter();
    if protocol.next().is_none_or(|value| value != "1") || protocol.next().is_some() {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}

async fn observe(State(state): State<Arc<StateData>>, request: Request, next: Next) -> Response {
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/search" => "search",
        "/v1/recordings/detail" => "detail",
        "/v1/sources" => "sources",
        "/v1/sources/prepare" => "prepare",
        "/v1/sources/preparation/status" => "preparation_status",
        "/v1/audio" => "audio",
        _ => "unknown",
    };
    let request_id = uuid::Uuid::new_v4();
    let span = tracing::debug_span!("all_debrid_plugin_request",%request_id,operation);
    let mut completed = RequestLog {
        dispatch: tracing::dispatcher::get_default(Clone::clone),
        span: span.clone(),
        started: Instant::now(),
        outcome: "cancelled",
        status: 0,
    };
    let result = async {
        if request
            .uri()
            .query()
            .is_some_and(|value| value.len() > 4096)
            || request.headers().len() > 32
            || request
                .headers()
                .iter()
                .map(|(key, value)| key.as_str().len() + value.as_bytes().len())
                .sum::<usize>()
                > 8192
        {
            return Err(Error::InvalidRequest);
        }
        if operation != "manifest" {
            authenticate(&state, request.headers())?;
        }
        let _permit = state
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        tokio::time::timeout(Duration::from_secs(14), next.run(request))
            .await
            .map_err(|_| Error::Deadline)
    }
    .instrument(span)
    .await;
    let mut response = result.unwrap_or_else(IntoResponse::into_response);
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    completed.status = response.status().as_u16();
    completed.outcome = match completed.status {
        200..=299 => "success",
        401 => "unauthorized",
        _ => "rejected",
    };
    response
}

struct RequestLog {
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    started: Instant,
    outcome: &'static str,
    status: u16,
}
impl Drop for RequestLog {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            event = "all_debrid_plugin_request_finished",
            outcome = self.outcome,
            status = self.status,
            elapsed_ms = self.started.elapsed().as_millis() as u64
        );
    }
}

pub(crate) enum Error {
    InvalidRequest,
    Unauthorized,
    NotFound,
    Conflict,
    Unavailable,
    Busy,
    Deadline,
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The request is invalid"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Plugin authorization is required"),
            Self::NotFound => (StatusCode::NOT_FOUND, "The requested source is unavailable"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "The operation belongs to another source",
            ),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "The source is unavailable"),
            Self::Busy => (StatusCode::TOO_MANY_REQUESTS, "The service is busy"),
            Self::Deadline => (
                StatusCode::GATEWAY_TIMEOUT,
                "The request exceeded its deadline",
            ),
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}
