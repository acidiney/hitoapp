use crate::{Config, InitError, catalog::Catalog, error::Error};
use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    middleware::from_fn_with_state,
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, LYRICS_V2_VERSION, LyricsCapabilitiesV2, LyricsRequest, LyricsResponseV2, Manifest,
    PROTOCOL_VERSION, ProviderRecording, RecordingMetadata, RecordingRequest, SearchPage,
    SearchRequest,
    server::{RequestPolicy, observe_requests, with_manifest},
};
use std::{future::Future, sync::Arc, time::Instant};

async fn observe<T>(
    operation: &'static str,
    work: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    let started = Instant::now();
    work.await
        .map_err(|error| error.observe(operation, started))
}

pub fn router(config: Config) -> Result<Router, InitError> {
    let policy = Arc::new(RequestPolicy::new(
        hito_plugin_sdk::request_telemetry!("lrclib"),
        Some(config.auth.clone()),
    ));
    let shutdown = config.shutdown_handle();
    let routes = Router::new()
        .route("/v1/search", post(search))
        .route("/v1/recordings/detail", post(detail))
        .route("/v1/recordings/metadata", post(metadata))
        .route(
            "/v2/lyrics/capabilities",
            get(|| async {
                Json(LyricsCapabilitiesV2 {
                    lyrics_version: LYRICS_V2_VERSION,
                })
            }),
        )
        .route("/v2/lyrics", post(lyrics));
    Ok(with_manifest(
        routes,
        Manifest {
            id: "org.hito.lrclib".into(),
            name: "LRCLIB lyrics".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Search, Capability::Metadata, Capability::Lyrics].into(),
            authorization: None,
            content_policy: None,
        },
        4096,
    )
    .with_state(Catalog::new(config)?)
    .layer(from_fn_with_state(policy, observe_requests))
    .layer(from_fn_with_state(shutdown, crate::shutdown::observe)))
}

#[tracing::instrument(level = "debug", skip_all, fields(operation = "metadata", request_id = %uuid::Uuid::new_v4()))]
async fn metadata(
    State(upstream): State<Catalog>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<RecordingMetadata>, Error> {
    observe("metadata", async {
        let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
        upstream.metadata(request).await.map(Json)
    })
    .await
}

#[tracing::instrument(level = "debug", skip_all, fields(operation = "lyrics_v2", request_id = %uuid::Uuid::new_v4()))]
async fn lyrics(
    State(upstream): State<Catalog>,
    payload: Result<Json<LyricsRequest>, JsonRejection>,
) -> Result<Json<LyricsResponseV2>, Error> {
    observe("lyrics", async {
        let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
        upstream.lyrics(request).await.map(Json)
    })
    .await
}

async fn detail(
    State(upstream): State<Catalog>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<ProviderRecording>, Error> {
    observe("detail", async {
        let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
        request.validate().map_err(|_| Error::InvalidRequest)?;
        upstream
            .record(&request.recording_id)
            .await?
            .recording()
            .map(Json)
    })
    .await
}

async fn search(
    State(upstream): State<Catalog>,
    payload: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, Error> {
    observe("search", async {
        let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
        upstream.search(request).await.map(Json)
    })
    .await
}
