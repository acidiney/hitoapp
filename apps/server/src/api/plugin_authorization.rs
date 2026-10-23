use super::plugins::PluginRevision;
use crate::{
    Config,
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{StatusCode, Uri},
};
use hito_core::{
    PluginAuthorization, PluginAuthorizationRedirect, PluginAuthorizationStatus, Services,
};

#[utoipa::path(post, path = "/api/v1/plugins/{id}/authorization", operation_id = "begin_plugin_authorization", params(("id" = String, Path)), request_body = PluginRevision, security(("session" = []), ("bearer" = [])), responses(
    (status = 201, body = PluginAuthorization), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn begin(
    State(services): State<Services>,
    Account(actor): Account,
    Extension(config): Extension<Config>,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<PluginRevision>,
) -> ApiResult<(
    StatusCode,
    [(&'static str, &'static str); 1],
    Json<PluginAuthorization>,
)> {
    let origin_uri: Uri = config.origin.parse().map_err(|_| invalid_origin())?;
    if origin_uri.scheme().is_none()
        || origin_uri.authority().is_none()
        || origin_uri.path() != "/"
        || origin_uri.query().is_some()
    {
        return Err(invalid_origin().into());
    }
    let origin = config.origin.strip_suffix('/').unwrap_or(&config.origin);
    let redirect = PluginAuthorizationRedirect::new(&format!(
        "{origin}/api/v1/plugin-authorization/callback"
    ))?;
    let attempt = services
        .plugins
        .begin_authorization(&actor, &id, request.revision, &redirect)
        .await?;
    Ok((
        StatusCode::CREATED,
        [("cache-control", "no-store")],
        Json(attempt),
    ))
}

#[utoipa::path(get, path = "/api/v1/plugins/{id}/authorization/{attempt}", operation_id = "plugin_authorization_status", params(("id" = String, Path), ("attempt" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PluginAuthorizationStatus), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn status(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, attempt)): Path<(String, String)>,
) -> ApiResult<(
    [(&'static str, &'static str); 1],
    Json<PluginAuthorizationStatus>,
)> {
    let status = services
        .plugins
        .authorization_status(&actor, &id, &attempt)
        .await?;
    Ok(([("cache-control", "no-store")], Json(status)))
}

fn invalid_origin() -> hito_core::Error {
    hito_core::Error::Invalid("Configure HITO_PUBLIC_ORIGIN as an HTTPS origin or a loopback IP over HTTP before connecting this provider".into())
}
