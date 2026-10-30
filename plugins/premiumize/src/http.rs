use crate::{
    Config, InitError, audio, config,
    config::Connection,
    policy::{AuthorizedConnection, Error},
    preparation,
};
use axum::{
    Json, Router,
    extract::rejection::JsonRejection,
    middleware::from_fn_with_state,
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, ContentPolicy, Manifest, PROTOCOL_VERSION, PreparationRequest,
    PreparationStatusRequest, PreparationStatusResponse, PreparationSubmission,
    PreparationSubmitResponse, ProviderRecording, ProviderSource, RecordingRequest, SearchPage,
    SearchRequest, SourcePage, SourcePersistence,
    server::{RequestPolicy, observe_requests, with_manifest},
};
use std::sync::Arc;

pub(crate) struct HttpState {
    pub(crate) connections: Vec<Arc<Connection>>,
}

pub fn router(config: Config) -> Result<Router, InitError> {
    let state = Arc::new(HttpState {
        connections: config::connections(config)?,
    });
    let policy = Arc::new(
        RequestPolicy::new(hito_plugin_sdk::request_telemetry!("premiumize"), None)
            .with_request_id_header(),
    );
    let routes = Router::new()
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/sources", post(sources))
        .route("/v1/sources/prepare", post(prepare))
        .route("/v1/sources/preparation/status", post(status))
        .route("/v1/audio", get(audio::response))
        .fallback(|| async { Error::NotFound });
    Ok(with_manifest(routes, manifest(), 4096)
        .layer(from_fn_with_state(policy, observe_requests))
        .with_state(state))
}

fn manifest() -> Manifest {
    Manifest {
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
    }
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
