use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{Json, extract::State};
use hito_core::{Genre, Services};

#[utoipa::path(get, path = "/api/v1/genres", operation_id = "genres",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Genre], headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn list(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<Genre>>> {
    Ok(Json(services.catalog.genres(&actor).await?))
}
