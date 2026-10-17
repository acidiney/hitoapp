use crate::{
    Config, InitError, audio::Relay, catalog::Catalog, error::Error, transport::Transport,
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Query, Request, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderMap, HeaderValue, Method},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, Manifest, PROTOCOL_VERSION, ProviderRecording, RecordingRequest, SearchPage,
    SearchRequest, SourcePage,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::Instrument;

struct HttpState {
    catalog: Arc<Catalog>,
    audio: Relay,
}

pub fn router(config: Config) -> Result<Router, InitError> {
    let transport = Arc::new(Transport::new(config)?);
    let catalog = Arc::new(Catalog::new(transport.clone()));
    let audio = Relay::new(catalog.clone(), transport);
    Ok(Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/sources", post(sources))
        .route("/v1/audio", get(audio_response))
        .layer(DefaultBodyLimit::max(4096))
        .layer(from_fn(observe))
        .with_state(Arc::new(HttpState { catalog, audio })))
}

async fn manifest() -> Json<Manifest> {
    Json(Manifest {
        id: "org.hito.jamendo".into(),
        name: "Jamendo music".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Search, Capability::Sources].into(),
        authorization: None,
    })
}

async fn search(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    state.catalog.search(request).await.map(Json)
}

async fn detail(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<ProviderRecording>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    state.catalog.detail(&request.recording_id).await.map(Json)
}

async fn sources(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<SourcePage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    state.catalog.sources(&request.recording_id).await.map(Json)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioQuery {
    source_id: String,
}

async fn audio_response(
    State(state): State<Arc<HttpState>>,
    query: Result<Query<AudioQuery>, QueryRejection>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let Query(request) = query.map_err(|_| Error::InvalidRequest)?;
    state.audio.open(&request.source_id, method, &headers).await
}

async fn observe(request: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4();
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/search" => "search",
        "/v1/recordings/detail" => "detail",
        "/v1/sources" => "sources",
        "/v1/audio" => "audio",
        _ => "unknown",
    };
    let span = tracing::debug_span!("jamendo_request", %request_id, operation);
    async move {
        let started = Instant::now();
        let mut response =
            match tokio::time::timeout(Duration::from_secs(22), next.run(request)).await {
                Ok(response) => response,
                Err(_) => Error::Deadline.into_response(),
            };
        response.headers_mut().insert(
            "cache-control",
            HeaderValue::from_static("private, no-store"),
        );
        tracing::debug!(
            event = "jamendo_request_complete",
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        response
    }
    .instrument(span)
    .await
}
