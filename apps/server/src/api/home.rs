use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{Json, extract::State};
use hito_core::{Home, Services};

#[utoipa::path(get, path = "/api/v1/home", operation_id = "home",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = Home, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn home(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Home>> {
    Ok(Json(services.collection.home(&actor).await?))
}
