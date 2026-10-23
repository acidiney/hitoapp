use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use hito_core::{Job, Services};

#[utoipa::path(post, path = "/api/v1/libraries/{library}/scan", operation_id = "scan_library", params(("library" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = [Job]), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn scan(
    State(services): State<Services>,
    Account(actor): Account,
    Path(library): Path<String>,
) -> ApiResult<(StatusCode, Json<Vec<Job>>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(services.storage.enqueue_scan(&actor, &library).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/jobs/{id}", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = Job), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn job(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<Job>> {
    Ok(Json(services.storage.job(&actor, &id).await?))
}
