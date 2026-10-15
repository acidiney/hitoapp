use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use hito_core::{
    InstallPlugin, OfferPluginService, PluginInstallation, PluginRefresh, PluginService,
    PluginSettings, RefreshPlugin, Services,
};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginRevision {
    pub revision: i64,
}

#[utoipa::path(get, path = "/api/v1/plugins", operation_id = "plugin_settings", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PluginSettings), (status = 401, body = ErrorResponse),
))]
pub async fn settings(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<PluginSettings>> {
    Ok(Json(services.plugins.settings(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/plugins/services", operation_id = "offer_plugin_service", request_body = OfferPluginService, security(("session" = []), ("bearer" = [])), responses(
    (status = 201, body = PluginService), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn offer(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<OfferPluginService>,
) -> ApiResult<(StatusCode, Json<PluginService>)> {
    Ok((
        StatusCode::CREATED,
        Json(services.plugins.offer_service(&actor, request).await?),
    ))
}

#[utoipa::path(delete, path = "/api/v1/plugins/services/{id}", operation_id = "withdraw_plugin_service", params(("id" = String, Path)), request_body = PluginRevision, security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn withdraw(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<PluginRevision>,
) -> ApiResult<StatusCode> {
    services
        .plugins
        .withdraw_service(&actor, &id, request.revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/plugins", operation_id = "install_plugin", request_body = InstallPlugin, security(("session" = []), ("bearer" = [])), responses(
    (status = 201, body = PluginInstallation), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn install(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<InstallPlugin>,
) -> ApiResult<(StatusCode, Json<PluginInstallation>)> {
    Ok((
        StatusCode::CREATED,
        Json(services.plugins.install(&actor, request).await?),
    ))
}

#[utoipa::path(post, path = "/api/v1/plugins/{id}/refresh", operation_id = "refresh_plugin", params(("id" = String, Path)), request_body = RefreshPlugin, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PluginRefresh), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn refresh(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<RefreshPlugin>,
) -> ApiResult<Json<PluginRefresh>> {
    Ok(Json(services.plugins.refresh(&actor, &id, request).await?))
}

#[utoipa::path(delete, path = "/api/v1/plugins/{id}", operation_id = "remove_plugin", params(("id" = String, Path)), request_body = PluginRevision, security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn remove(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<PluginRevision>,
) -> ApiResult<StatusCode> {
    services
        .plugins
        .remove(&actor, &id, request.revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/plugins/{id}/credential", operation_id = "clear_plugin_credential", params(("id" = String, Path)), request_body = PluginRevision, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PluginInstallation), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn clear_credential(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<PluginRevision>,
) -> ApiResult<Json<PluginInstallation>> {
    Ok(Json(
        services
            .plugins
            .clear_credential(&actor, &id, request.revision)
            .await?,
    ))
}
