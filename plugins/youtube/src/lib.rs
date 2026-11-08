mod artwork;
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
mod classify;
mod extractor;
mod media;
mod process;
mod recording;
mod recordings;

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Query, Request, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderMap, Method, StatusCode},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    ArtworkRequest, Capability, Manifest, PROTOCOL_VERSION, ProviderRecording, ProviderSource,
    RecordingMetadata, RecordingRequest, SearchPage, SearchRequest, SourcePage,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use tracing::Instrument;

pub struct Config {
    pub executable: PathBuf,
}

pub struct Service {
    state: Arc<HttpState>,
}

struct HttpState {
    extractor: Arc<extractor::Extractor>,
    catalog: Arc<recording::Catalog>,
    media: Arc<media::Relay>,
    artwork: artwork::Fetcher,
    audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownError {
    CleanupFailed,
}

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("The YouTube service could not finish process cleanup")
    }
}

impl std::error::Error for ShutdownError {}

impl Service {
    pub fn new(config: Config) -> Self {
        Self::configured(config, media::Client::new(), true)
    }

    pub fn metadata_only(config: Config) -> Self {
        Self::configured(config, media::Client::new(), false)
    }

    #[cfg(test)]
    fn with_media_client(config: Config, client: media::Client) -> Self {
        Self::configured(config, Ok(client), true)
    }

    fn configured(config: Config, client: Result<media::Client, Error>, audio: bool) -> Self {
        let extractor = Arc::new(extractor::Extractor::new(config.executable));
        let catalog = Arc::new(recording::Catalog::new(extractor.clone()));
        let artwork = artwork::Fetcher::new(client.clone());
        let media = Arc::new(media::Relay::new(catalog.clone(), client));
        Self {
            state: Arc::new(HttpState {
                extractor,
                catalog,
                media,
                artwork,
                audio,
            }),
        }
    }

    pub fn router(&self) -> Router {
        let router = Router::new()
            .route("/v1/manifest", get(manifest))
            .route("/v1/search", post(search))
            .route("/v1/recordings/detail", post(detail))
            .route("/v1/recordings/metadata", post(metadata))
            .route("/v1/artwork", post(artwork));
        let router = if self.state.audio {
            router
                .route("/v1/sources", post(sources))
                .route("/v1/audio", get(audio))
        } else {
            router
        };
        router
            .layer(DefaultBodyLimit::max(4096))
            .layer(from_fn(observe))
            .with_state(self.state.clone())
    }

    pub async fn shutdown(&self) -> Result<(), ShutdownError> {
        self.state.artwork.shutdown();
        let (cleanup, ()) =
            tokio::join!(self.state.extractor.shutdown(), self.state.media.shutdown());
        cleanup.map_err(|_| ShutdownError::CleanupFailed)
    }
}

pub fn router(config: Config) -> Router {
    Service::new(config).router()
}

async fn manifest(State(state): State<Arc<HttpState>>) -> Json<Manifest> {
    let mut capabilities: std::collections::BTreeSet<_> = [
        Capability::Search,
        Capability::Sources,
        Capability::Metadata,
        Capability::Artwork,
    ]
    .into();
    if !state.audio {
        capabilities.remove(&Capability::Sources);
    }
    Json(Manifest {
        id: "org.hito.youtube".into(),
        name: "YouTube".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities,
        authorization: None,
        content_policy: None,
    })
}

async fn search(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    if request.cursor.is_some() {
        return Err(Error::InvalidRequest);
    }
    state.extractor.search(request).await.map(Json)
}

async fn detail(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<ProviderRecording>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRecordingRequest)?;
    request
        .validate()
        .map_err(|_| Error::InvalidRecordingRequest)?;
    let resolved = state.catalog.recording(&request.recording_id).await?;
    Ok(Json(resolved.recording.clone()))
}

async fn sources(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<SourcePage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRecordingRequest)?;
    request
        .validate()
        .map_err(|_| Error::InvalidRecordingRequest)?;
    let resolved = state.catalog.recording(&request.recording_id).await?;
    Ok(Json(SourcePage {
        items: vec![ProviderSource {
            id: resolved.source.id.clone(),
            label: "AAC / M4A".into(),
            ready: true,
        }],
    }))
}

async fn metadata(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<RecordingRequest>, JsonRejection>,
) -> Result<Json<RecordingMetadata>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRecordingRequest)?;
    request
        .validate()
        .map_err(|_| Error::InvalidRecordingRequest)?;
    let resolved = state.catalog.recording(&request.recording_id).await?;
    resolved.metadata().map(Json)
}

async fn artwork(
    State(state): State<Arc<HttpState>>,
    payload: Result<Json<ArtworkRequest>, JsonRejection>,
) -> Result<Response, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidArtworkRequest)?;
    let image = state.artwork.fetch(request).await?;
    Ok((
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "private, no-store"),
        ],
        image,
    )
        .into_response())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioQuery {
    source_id: String,
}

async fn audio(
    State(state): State<Arc<HttpState>>,
    query: Result<Query<AudioQuery>, QueryRejection>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let Query(request) = query.map_err(|_| Error::InvalidAudioRequest)?;
    if request.source_id.len() > 256 {
        return Err(Error::InvalidAudioRequest);
    }
    state.media.open(&request.source_id, method, headers).await
}

async fn observe(request: Request, next: Next) -> Response {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let request_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/search" => "search",
        "/v1/recordings/detail" => "detail",
        "/v1/recordings/metadata" => "metadata",
        "/v1/sources" => "sources",
        "/v1/audio" => "audio",
        "/v1/artwork" => "artwork",
        _ => "unknown",
    };
    let span = tracing::info_span!("youtube_request", request_id, operation);
    async move {
        let started = Instant::now();
        let response = next.run(request).await;
        tracing::info!(
            event = "youtube_request_complete",
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        response
    }
    .instrument(span)
    .await
}

#[derive(Clone, Copy)]
pub(crate) enum Error {
    InvalidRequest,
    InvalidRecordingRequest,
    InvalidAudioRequest,
    InvalidArtworkRequest,
    InvalidMedia,
    UpstreamRejected,
    ValidatorChanged,
    NotFound,
    VideoUnavailable,
    FormatMissing,
    Refused,
    Network,
    Busy,
    Unavailable,
    InvalidResponse,
    Deadline,
    Canceled,
    CleanupFailed,
}

impl Error {
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidRecordingRequest
            | Self::InvalidAudioRequest
            | Self::InvalidArtworkRequest => "invalid_request",
            Self::InvalidMedia => "invalid_response",
            Self::UpstreamRejected => "upstream_rejected",
            Self::ValidatorChanged => "validator_changed",
            Self::NotFound => "not_found",
            Self::VideoUnavailable => "video_unavailable",
            Self::FormatMissing => "format_missing",
            Self::Refused => "refused",
            Self::Network => "network",
            Self::Busy => "capacity",
            Self::Unavailable => "unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::Deadline => "deadline",
            Self::Canceled => "canceled",
            Self::CleanupFailed => "cleanup_failed",
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The search request is invalid"),
            Self::InvalidRecordingRequest => {
                (StatusCode::BAD_REQUEST, "The recording request is invalid")
            }
            Self::InvalidAudioRequest => (StatusCode::BAD_REQUEST, "The audio request is invalid"),
            Self::InvalidArtworkRequest => {
                (StatusCode::BAD_REQUEST, "The artwork request is invalid")
            }
            Self::InvalidMedia | Self::UpstreamRejected | Self::ValidatorChanged => (
                StatusCode::BAD_GATEWAY,
                "The YouTube audio response is invalid",
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "The YouTube source was not found"),
            Self::VideoUnavailable => (StatusCode::NOT_FOUND, "The YouTube video is unavailable"),
            Self::FormatMissing => (
                StatusCode::NOT_FOUND,
                "The YouTube video has no supported audio format",
            ),
            Self::Refused => (
                StatusCode::SERVICE_UNAVAILABLE,
                "YouTube refused the request",
            ),
            Self::Network => (StatusCode::SERVICE_UNAVAILABLE, "Hito cannot reach YouTube"),
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The YouTube service is busy. Try again later",
            ),
            Self::Unavailable | Self::Canceled | Self::CleanupFailed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The YouTube service is unavailable",
            ),
            Self::InvalidResponse => (
                StatusCode::BAD_GATEWAY,
                "YouTube returned an invalid search response",
            ),
            Self::Deadline => (StatusCode::GATEWAY_TIMEOUT, "The YouTube search timed out"),
        };
        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}

#[cfg(all(test, target_os = "linux", not(target_env = "uclibc")))]
mod tests;
