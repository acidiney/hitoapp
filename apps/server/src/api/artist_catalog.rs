use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{Json, extract::State};
use hito_core::{MetadataSettings, Services, UpdateAutomaticLookups, UpdateMetadataSettings};

#[utoipa::path(get, path = "/api/v1/metadata/settings", operation_id = "metadata_settings",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = MetadataSettings), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn settings(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<MetadataSettings>> {
    Ok(Json(
        services.artist_catalog.metadata_settings(&actor).await?,
    ))
}

#[utoipa::path(put, path = "/api/v1/metadata/settings", operation_id = "update_metadata_settings",
    request_body = UpdateMetadataSettings, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = MetadataSettings), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn update_settings(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<UpdateMetadataSettings>,
) -> ApiResult<Json<MetadataSettings>> {
    Ok(Json(
        services
            .artist_catalog
            .set_metadata_settings(&actor, request)
            .await?,
    ))
}

#[utoipa::path(put, path = "/api/v1/metadata/automation", operation_id = "update_automatic_lookups",
    request_body = UpdateAutomaticLookups, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = MetadataSettings), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn update_automation(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<UpdateAutomaticLookups>,
) -> ApiResult<Json<MetadataSettings>> {
    Ok(Json(
        services
            .artist_catalog
            .set_automatic_lookups(&actor, request)
            .await?,
    ))
}
