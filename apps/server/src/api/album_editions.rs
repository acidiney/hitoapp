use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{AlbumEditionTrackListAvailability, ReadAlbumEditionTrackList, Services};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/albums/{id}/edition-track-list", operation_id = "album_edition_track_list",
    params(("id" = String, Path)),
    request_body = ReadAlbumEditionTrackList,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumEditionTrackListAvailability, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_album_edition_track_list", request_id = %Uuid::new_v4()))]
pub async fn track_list(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ReadAlbumEditionTrackList>,
) -> ApiResult<Json<AlbumEditionTrackListAvailability>> {
    let started = Instant::now();
    let result = services
        .catalog
        .album_edition_track_list_until(&actor, &id, request, std::future::pending())
        .await;
    tracing::debug!(
        outcome = match &result {
            Ok(AlbumEditionTrackListAvailability::Available { .. }) => "available",
            Ok(AlbumEditionTrackListAvailability::Disabled) => "disabled",
            Ok(AlbumEditionTrackListAvailability::Unsupported) => "unsupported",
            Ok(AlbumEditionTrackListAvailability::Unavailable) => "unavailable",
            Ok(AlbumEditionTrackListAvailability::Cancelled) => "cancelled",
            Ok(AlbumEditionTrackListAvailability::NeedsChoice) => "needs_choice",
            Err(error) => error.code(),
        },
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Album edition track list HTTP request completed"
    );
    Ok(Json(result?))
}
