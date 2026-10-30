use crate::{Config, InitError, audio, catalog::Catalog, error::Error};
use axum::{
    Json, Router,
    extract::{Request, State, rejection::JsonRejection},
    http::HeaderValue,
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_debrid_real_debrid::{MediaAllowlist, RealDebridClient, TorrentPhase};
use hito_plugin_sdk::{
    Capability, ContentPolicy, Manifest, PROTOCOL_VERSION, ProviderRecording, ProviderSource,
    RecordingRequest, SearchPage, SearchRequest, SourcePage, SourcePersistence,
    server::{ServiceAuth, with_manifest},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tracing::Instrument;

pub(crate) struct HttpState {
    pub(crate) authentication: ServiceAuth,
    pub(crate) catalog: Catalog,
    pub(crate) client: RealDebridClient,
    pub(crate) media_origins: MediaAllowlist,
    request_slots: Arc<Semaphore>,
}

pub fn router(config: Config) -> Result<Router, InitError> {
    let state = Arc::new(HttpState {
        authentication: config.authentication,
        catalog: config.catalog,
        client: config.client,
        media_origins: config.media_origins,
        request_slots: Arc::new(Semaphore::new(16)),
    });
    let routes = Router::new()
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/sources", post(sources))
        .route("/v1/audio", get(audio::response));
    Ok(with_manifest(routes, manifest(), 4096)
        .layer(from_fn_with_state(state.clone(), observe))
        .with_state(state))
}

fn manifest() -> Manifest {
    Manifest {
        id: "org.hito.real-debrid".into(),
        name: "Real-Debrid curated audio".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Search, Capability::Sources].into(),
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
    Ok(Json(
        state
            .catalog
            .recording(&request.recording_id)?
            .recording
            .clone(),
    ))
}

async fn sources(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<SourcePage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let entry = state.catalog.recording(&request.recording_id)?;
    let observation = state
        .client
        .observe(&entry.binding)
        .await
        .map_err(Error::from)?;
    Ok(Json(SourcePage {
        items: vec![ProviderSource {
            id: entry.source_id.clone(),
            label: entry.label.clone(),
            ready: observation.phase == TorrentPhase::ProviderCompleted,
        }],
    }))
}

async fn observe(State(state): State<Arc<HttpState>>, request: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4();
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/search" => "search",
        "/v1/recordings/detail" => "detail",
        "/v1/sources" => "sources",
        "/v1/audio" => "audio",
        _ => "unknown",
    };
    let span = tracing::debug_span!("real_debrid_plugin_request", %request_id, operation);
    async move {
        let started = Instant::now();
        let response = async {
            if operation != "manifest" {
                state
                    .authentication
                    .check(request.headers())
                    .map_err(Error::from)?;
            }
            if request
                .uri()
                .query()
                .is_some_and(|query| query.len() > 4096)
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
                .request_slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::Busy)?;
            tokio::time::timeout(Duration::from_secs(13), next.run(request))
                .await
                .map_err(|_| Error::Deadline)
        }
        .await;
        let mut response = response.unwrap_or_else(IntoResponse::into_response);
        response.headers_mut().insert(
            "cache-control",
            HeaderValue::from_static("private, no-store"),
        );
        response.headers_mut().insert(
            "x-content-type-options",
            HeaderValue::from_static("nosniff"),
        );
        let outcome = match response.status().as_u16() {
            200..=299 => "success",
            401 => "unauthorized",
            _ => "rejected",
        };
        tracing::debug!(
            event = "real_debrid_plugin_request_finished",
            outcome,
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        response
    }
    .instrument(span)
    .await
}
