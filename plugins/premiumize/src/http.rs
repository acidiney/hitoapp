use crate::{
    Config, InitError, audio, config,
    config::Connection,
    policy::{AuthorizedConnection, Error},
    preparation,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::HeaderValue,
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, ContentPolicy, Manifest, PROTOCOL_VERSION, PreparationRequest,
    PreparationStatusRequest, PreparationStatusResponse, PreparationSubmission,
    PreparationSubmitResponse, ProviderRecording, ProviderSource, RecordingRequest, SearchPage,
    SearchRequest, SourcePage, SourcePersistence,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tracing::Instrument;

pub(crate) struct HttpState {
    pub(crate) connections: Vec<Arc<Connection>>,
    slots: Arc<Semaphore>,
}

pub fn router(config: Config) -> Result<Router, InitError> {
    let state = Arc::new(HttpState {
        connections: config::connections(config)?,
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
        .fallback(|| async { Error::NotFound })
        .layer(DefaultBodyLimit::max(4096))
        .layer(from_fn_with_state(state.clone(), observe))
        .with_state(state))
}

async fn manifest() -> Json<Manifest> {
    Json(Manifest {
        id: "org.hito.premiumize".into(),
        name: "Premiumize curated WAV".into(),
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
    AuthorizedConnection(connection): AuthorizedConnection,
    payload: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    connection.catalog.search(request).map(Json)
}

async fn detail(
    AuthorizedConnection(connection): AuthorizedConnection,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<ProviderRecording>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    Ok(Json(
        connection
            .catalog
            .recording(&request.recording_id)?
            .recording
            .clone(),
    ))
}

async fn sources(
    AuthorizedConnection(connection): AuthorizedConnection,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<SourcePage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let entry = connection.catalog.recording(&request.recording_id)?;
    let ready = preparation::ready(&connection, entry).await?;
    Ok(Json(SourcePage {
        items: vec![ProviderSource {
            id: entry.source_id.clone(),
            label: entry.label.clone(),
            ready,
        }],
    }))
}

async fn prepare(
    AuthorizedConnection(connection): AuthorizedConnection,
    payload: Result<Json<PreparationRequest>, JsonRejection>,
) -> Result<Json<PreparationSubmitResponse>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let entry = connection
        .catalog
        .target(&request.recording_id, &request.source_id)?;
    let handle = connection.receipts.issue(request.operation_id, entry)?;
    let status = preparation::status(&connection, entry).await?;
    Ok(Json(PreparationSubmitResponse {
        operation_id: request.operation_id,
        recording_id: request.recording_id,
        source_id: request.source_id,
        submission: PreparationSubmission::Accepted { handle, status },
    }))
}

async fn status(
    AuthorizedConnection(connection): AuthorizedConnection,
    payload: Result<Json<PreparationStatusRequest>, JsonRejection>,
) -> Result<Json<PreparationStatusResponse>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let entry = connection
        .catalog
        .target(&request.recording_id, &request.source_id)?;
    connection
        .receipts
        .verify(request.operation_id, entry, &request.handle)?;
    let status = preparation::status(&connection, entry).await?;
    Ok(Json(PreparationStatusResponse {
        operation_id: request.operation_id,
        recording_id: request.recording_id,
        source_id: request.source_id,
        handle: request.handle,
        status,
    }))
}

async fn observe(State(state): State<Arc<HttpState>>, request: Request, next: Next) -> Response {
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
    let span = tracing::debug_span!("premiumize_plugin_request", %request_id, operation);
    let mut log = RequestLog {
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
                .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
                .sum::<usize>()
                > 8192
        {
            return Err(Error::InvalidRequest);
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
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id.to_string())
            .expect("a generated request UUID is a valid header value"),
    );
    log.status = response.status().as_u16();
    log.outcome = match log.status {
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
            event = "premiumize_plugin_request_finished",
            outcome = self.outcome,
            status = self.status,
            elapsed_ms = self.started.elapsed().as_millis() as u64
        );
    }
}
