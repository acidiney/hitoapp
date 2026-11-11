use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{AutomaticLyricsLookup, ReadAutomaticRecordingLyrics, Services};

#[utoipa::path(post, path = "/api/v1/recordings/{id}/lyrics/automatic", operation_id = "automatic_recording_lyrics",
    params(("id" = String, Path)), request_body = ReadAutomaticRecordingLyrics,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AutomaticLyricsLookup, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(_request): JsonBody<ReadAutomaticRecordingLyrics>,
) -> ApiResult<Json<AutomaticLyricsLookup>> {
    Ok(Json(
        services
            .catalog
            .automatic_recording_lyrics_until(&actor, &id, std::future::pending())
            .await?,
    ))
}
