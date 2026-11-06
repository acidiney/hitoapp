use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{Json, extract::State};
use hito_core::{Services, SourcePreferences, UpdateSourcePreferences};

#[utoipa::path(get, path = "/api/v1/source-preferences", operation_id = "source_preferences",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SourcePreferences), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn source_preferences(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<SourcePreferences>> {
    Ok(Json(services.catalog.source_preferences(&actor).await?))
}

#[utoipa::path(put, path = "/api/v1/source-preferences", operation_id = "update_source_preferences",
    request_body = UpdateSourcePreferences, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SourcePreferences), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn update_source_preferences(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<UpdateSourcePreferences>,
) -> ApiResult<Json<SourcePreferences>> {
    Ok(Json(
        services
            .catalog
            .update_source_preferences(&actor, request)
            .await?,
    ))
}
