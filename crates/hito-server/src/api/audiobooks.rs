use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{AudiobookDetail, Playlist, Services};

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
