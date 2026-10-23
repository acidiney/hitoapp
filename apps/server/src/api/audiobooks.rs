use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::QueryParams,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{
    AudiobookDetail, AudiobookLibraryPage, AudiobookLibraryQuery, AudiobookPlayback, Playlist,
    Services,
};

#[utoipa::path(get, path = "/api/v1/audiobooks", operation_id = "audiobooks",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Playlist], headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn list(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<Playlist>>> {
    Ok(Json(services.collection.audiobooks(&actor).await?))
}

#[utoipa::path(get, path = "/api/v1/audiobooks/{id}", operation_id = "audiobook",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AudiobookDetail, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn detail(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<AudiobookDetail>> {
    Ok(Json(services.collection.audiobook(&actor, &id).await?))
}

#[utoipa::path(get, path = "/api/v1/audiobooks/{id}/playback", operation_id = "audiobook_playback",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AudiobookPlayback, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn playback(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<AudiobookPlayback>> {
    Ok(Json(
        services.collection.audiobook_playback(&actor, &id).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/audiobooks/page", operation_id = "audiobook_library",
    params(AudiobookLibraryQuery),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AudiobookLibraryPage, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn page(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<AudiobookLibraryQuery>,
) -> ApiResult<Json<AudiobookLibraryPage>> {
    Ok(Json(
        services.collection.audiobook_library(&actor, query).await?,
    ))
}
