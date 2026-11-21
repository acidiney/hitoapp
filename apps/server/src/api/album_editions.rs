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
    AddAlbum, AlbumAddition, AlbumAdditionUndone, AlbumEditionChoiceReceipt,
    AlbumEditionTrackListAvailability, AlbumLibraryLevel, AlbumTrackAudio, ChooseAlbumEdition,
    ClearAlbumEdition, FindAlbumTrackAudio, ReadAlbumEditionTrackList, ReadAlbumLibraryLevel,
    Services, TaggedAlbumEdition,
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

#[utoipa::path(post, path = "/api/v1/albums/{id}/tagged-edition", operation_id = "tagged_album_edition",
    params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = TaggedAlbumEdition, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_tagged_album_edition", request_id = %Uuid::new_v4()))]
pub async fn tagged_edition(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<TaggedAlbumEdition>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .tagged_album_edition_until(&actor, &id, std::future::pending())
        .await;
    tracing::debug!(
        outcome = result.as_ref().map_or_else(
            |error| error.code(),
            |found| match found {
                TaggedAlbumEdition::Found { .. } => "found",
                TaggedAlbumEdition::None => "none",
            }
        ),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Tagged album edition HTTP request completed"
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

#[utoipa::path(post, path = "/api/v1/albums/{id}/additions", operation_id = "add_album",
    description = "Add every audio track of the chosen edition that this account does not own yet, at once and without audio. Repeating a request returns the same addition.",
    params(("id" = String, Path)),
    request_body = AddAlbum,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumAddition, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_add_album"))]
pub async fn add(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<AddAlbum>,
) -> ApiResult<Json<AlbumAddition>> {
    Ok(Json(
        services
            .artist_catalog
            .add_album(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/albums/additions/{addition}", operation_id = "undo_album_addition",
    description = "Undo an album addition. Tracks used since (liked, in a playlist, played or with audio) stay; the rest are removed. Undoing twice changes nothing.",
    params(("addition" = Uuid, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumAdditionUndone, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_undo_album_addition")
)]
pub async fn undo(
    State(services): State<Services>,
    Account(actor): Account,
    Path(addition): Path<Uuid>,
) -> ApiResult<Json<AlbumAdditionUndone>> {
    Ok(Json(
        services
            .artist_catalog
            .undo_album_addition(&actor, addition)
            .await?,
    ))
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

#[utoipa::path(post, path = "/api/v1/albums/{id}/edition/library-level", operation_id = "album_library_level",
    params(("id" = String, Path)), request_body = ReadAlbumLibraryLevel,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AlbumLibraryLevel, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_album_library_level", request_id = %Uuid::new_v4()))]
pub async fn library_level(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ReadAlbumLibraryLevel>,
) -> ApiResult<Json<AlbumLibraryLevel>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .album_library_level_until(&actor, &id, request, std::future::pending())
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), AlbumLibraryLevel::outcome),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Album library level HTTP request completed"
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
