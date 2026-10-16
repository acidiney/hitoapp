mod extractor;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::StatusCode,
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::post,
};
use hito_plugin_sdk::{SearchPage, SearchRequest};
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

pub fn router(config: Config) -> Router {
    Router::new()
        .route("/v1/search", post(search))
        .layer(DefaultBodyLimit::max(4096))
        .layer(from_fn(observe))
        .with_state(Arc::new(extractor::Extractor::new(config.executable)))
}

async fn search(
    State(extractor): State<Arc<extractor::Extractor>>,
    payload: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    request.validate().map_err(|_| Error::InvalidRequest)?;
    if request.cursor.is_some() {
        return Err(Error::InvalidRequest);
    }
    extractor.search(request).await.map(Json)
}

async fn observe(request: Request, next: Next) -> Response {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let request_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let operation = match request.uri().path() {
        "/v1/search" => "search",
        _ => "unknown",
    };
    let span = tracing::info_span!("youtube_request", request_id, operation);
    async move {
        let started = Instant::now();
        let response = next.run(request).await;
        tracing::debug!(
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
    Busy,
    Unavailable,
    InvalidResponse,
    Deadline,
    Canceled,
}

impl Error {
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::Busy => "capacity",
            Self::Unavailable => "unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::Deadline => "deadline",
            Self::Canceled => "canceled",
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The search request is invalid"),
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The YouTube service is busy. Try again later",
            ),
            Self::Unavailable | Self::Canceled => (
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

#[cfg(all(test, unix))]
mod tests;
