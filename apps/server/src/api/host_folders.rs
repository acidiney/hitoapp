use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{Extension, Json};
use futures_util::future::BoxFuture;
use hito_core::{DirectoryRef, Error, Services};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use utoipa::ToSchema;

pub trait HostFolders: Send + Sync + 'static {
    fn suggestion(&self) -> BoxFuture<'_, Option<PathBuf>>;
    fn choose(&self) -> BoxFuture<'_, Option<PathBuf>>;
}

#[derive(Clone)]
pub(crate) struct HostFolderAccess(pub(crate) Option<Arc<dyn HostFolders>>);

#[derive(Serialize, ToSchema)]
pub struct HostFolderOffer {
    pub picker: bool,
    pub suggestion: Option<DirectoryRef>,
}

#[derive(Serialize, ToSchema)]
pub struct HostFolderChoice {
    pub directory: Option<DirectoryRef>,
}

#[utoipa::path(get, path = "/api/v1/host/folders", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = HostFolderOffer), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn offer(
    axum::extract::State(services): axum::extract::State<Services>,
    Account(actor): Account,
    Extension(host): Extension<HostFolderAccess>,
) -> ApiResult<Json<HostFolderOffer>> {
    let Some(host) = host.0 else {
        return Ok(Json(HostFolderOffer {
            picker: false,
            suggestion: None,
        }));
    };
    let path = host.suggestion().await;
    let suggestion = services
        .storage
        .find_host_directory(&actor, path.as_deref())
        .await?;
    Ok(Json(HostFolderOffer {
        picker: true,
        suggestion,
    }))
}

#[utoipa::path(post, path = "/api/v1/host/folders/choose", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = HostFolderChoice), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn choose(
    axum::extract::State(services): axum::extract::State<Services>,
    Account(actor): Account,
    Extension(host): Extension<HostFolderAccess>,
) -> ApiResult<Json<HostFolderChoice>> {
    let host = host.0.ok_or(Error::NotFound)?;
    services.storage.find_host_directory(&actor, None).await?;
    let directory = match host.choose().await {
        Some(path) => Some(services.storage.grant_host_directory(&actor, &path).await?),
        None => None,
    };
    tracing::info!(
        event = "host_folder_chosen",
        chosen = directory.is_some(),
        "Native folder selection finished"
    );
    Ok(Json(HostFolderChoice { directory }))
}
