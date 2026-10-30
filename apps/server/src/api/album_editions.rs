use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{
    AlbumEditionChoiceReceipt, AlbumEditionTrackListAvailability, AlbumTrackAudio,
    ChooseAlbumEdition, ClearAlbumEdition, FindAlbumTrackAudio, ReadAlbumEditionTrackList,
    Services,
};
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
        .artist_catalog
        .album_edition_track_list_until(&actor, &id, request, std::future::pending())
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |read| read.outcome()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Album edition track list HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/albums/{id}/edition-track-audio", operation_id = "album_edition_track_audio",
    params(("id" = String, Path)),
    request_body = FindAlbumTrackAudio,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumTrackAudio, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_album_edition_track_audio", request_id = %Uuid::new_v4()))]
pub async fn track_audio(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<FindAlbumTrackAudio>,
) -> ApiResult<Json<AlbumTrackAudio>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .find_album_track_audio_until(&actor, &id, request, std::future::pending())
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |found| found.outcome()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Album track audio HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/albums/{id}/edition/track-list", operation_id = "saved_album_edition_track_list",
    params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumEditionTrackListAvailability, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_saved_album_edition_track_list", request_id = %Uuid::new_v4()))]
pub async fn saved_track_list(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<AlbumEditionTrackListAvailability>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .saved_album_edition_track_list_until(&actor, &id, std::future::pending())
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |read| read.outcome()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Saved album edition track list HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(put, path = "/api/v1/albums/{id}/edition", operation_id = "choose_album_edition",
    params(("id" = String, Path)), request_body = ChooseAlbumEdition,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumEditionChoiceReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_album_edition_choose", request_id = %Uuid::new_v4()))]
pub async fn choose(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ChooseAlbumEdition>,
) -> ApiResult<Json<AlbumEditionChoiceReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .choose_album_edition(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Album edition choice HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/albums/{id}/edition/clear", operation_id = "clear_album_edition",
    params(("id" = String, Path)), request_body = ClearAlbumEdition,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumEditionChoiceReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_album_edition_clear", request_id = %Uuid::new_v4()))]
pub async fn clear(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ClearAlbumEdition>,
) -> ApiResult<Json<AlbumEditionChoiceReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .clear_album_edition(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Album edition clear HTTP request completed"
    );
    Ok(Json(result?))
}
