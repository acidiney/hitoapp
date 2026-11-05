use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Extension,
    extract::{Path, State},
    http::StatusCode,
};
use futures_util::future::BoxFuture;
use hito_core::{Error, Services};
use serde::Deserialize;
use std::sync::Arc;
use url::Url;
use utoipa::ToSchema;

pub enum HostOpen {
    Opened,
    Failed,
}

pub trait HostBrowser: Send + Sync + 'static {
    fn open(&self, url: Url) -> BoxFuture<'_, HostOpen>;
}

#[derive(Clone)]
pub(crate) struct HostBrowserAccess(pub(crate) Option<Arc<dyn HostBrowser>>);

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationLaunch {
    pub authorization_url: String,
}

#[utoipa::path(post, path = "/api/v1/plugins/{id}/authorization/{attempt}/launch", operation_id = "launch_plugin_authorization", params(("id" = String, Path), ("attempt" = String, Path)), request_body = AuthorizationLaunch, security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn launch(
    State(services): State<Services>,
    Account(actor): Account,
    Extension(host): Extension<HostBrowserAccess>,
    Path((id, attempt)): Path<(String, String)>,
    JsonBody(request): JsonBody<AuthorizationLaunch>,
) -> ApiResult<StatusCode> {
    let browser = host.0.ok_or(Error::NotFound)?;
    let url = services
        .plugins
        .authorization_launch(&actor, &id, &attempt, &request.authorization_url)
        .await?;
    match browser.open(url).await {
        HostOpen::Opened => {
            tracing::info!(
                event = "host_browser_opened",
                attempt_id = attempt,
                "Provider sign-in page handed to the system browser"
            );
            Ok(StatusCode::NO_CONTENT)
        }
        HostOpen::Failed => {
            tracing::warn!(
                event = "host_browser_failed",
                attempt_id = attempt,
                "The system browser could not be opened"
            );
            Err(Error::Upstream("The system browser could not be opened").into())
        }
    }
}
