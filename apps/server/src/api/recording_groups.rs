use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{RecordingCopies, RecordingSplit, Services};

#[utoipa::path(get, path = "/api/v1/recordings/{id}/copies", operation_id = "recording_copies",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingCopies), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn copies(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<RecordingCopies>> {
    Ok(Json(services.catalog.recording_copies(&actor, &id).await?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/copies/split", operation_id = "split_recording_copy",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingSplit), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse),
))]
pub async fn split(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<RecordingSplit>> {
    Ok(Json(
        services.catalog.split_recording_copy(&actor, &id).await?,
    ))
}
