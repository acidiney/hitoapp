use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{LyricsLookup, ReadRecordingLyrics, RecordingLyricsProviders, Services};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(get, path = "/api/v1/recordings/{id}/lyrics/providers", operation_id = "recording_lyrics_providers",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingLyricsProviders, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn providers(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<RecordingLyricsProviders>> {
    Ok(Json(
        services
            .catalog
            .recording_lyrics_providers(&actor, &id)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/lyrics", operation_id = "recording_lyrics",
    params(("id" = String, Path)), request_body = ReadRecordingLyrics,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = LyricsLookup, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_lyrics", request_id = %Uuid::new_v4()))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ReadRecordingLyrics>,
) -> ApiResult<Json<LyricsLookup>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_lyrics_until(&actor, &id, request, std::future::pending())
        .await;
    let outcome = match &result {
        Ok(LyricsLookup::Ready { document, .. }) if document.lyrics.is_none() => "not_found",
        Ok(LyricsLookup::Ready { .. }) => "ready",
        Ok(LyricsLookup::Unsupported) => "unsupported",
        Ok(LyricsLookup::Unavailable) => "unavailable",
        Ok(LyricsLookup::Cancelled) => "cancelled",
        Err(error) => error.code(),
    };
    tracing::debug!(
        outcome,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording lyric HTTP request completed"
    );
    Ok(Json(result?))
}
