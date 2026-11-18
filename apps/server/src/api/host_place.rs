use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{Extension, Json, http::StatusCode};
use futures_util::future::BoxFuture;
use hito_core::Error;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostPlaceCurrent {
    ThisComputer,
}

pub trait HostPlace: Send + Sync + 'static {
    fn current(&self) -> HostPlaceCurrent;
    fn change(&self) -> BoxFuture<'_, ()>;
}

#[derive(Clone)]
pub(crate) struct HostPlaceAccess(pub(crate) Option<Arc<dyn HostPlace>>);

#[derive(Serialize, ToSchema)]
pub struct HostPlaceOffer {
    pub changeable: bool,
    pub current: Option<HostPlaceCurrent>,
}

#[utoipa::path(get, path = "/api/v1/host/place", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = HostPlaceOffer), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn offer(
    Account(_actor): Account,
    Extension(host): Extension<HostPlaceAccess>,
) -> ApiResult<Json<HostPlaceOffer>> {
    Ok(Json(match host.0 {
        Some(host) => HostPlaceOffer {
            changeable: true,
            current: Some(host.current()),
        },
        None => HostPlaceOffer {
            changeable: false,
            current: None,
        },
    }))
}

#[utoipa::path(post, path = "/api/v1/host/place/change", security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse),
))]
pub async fn change(
    Account(_actor): Account,
    Extension(host): Extension<HostPlaceAccess>,
) -> ApiResult<StatusCode> {
    let host = host.0.ok_or(Error::NotFound)?;
    host.change().await;
    tracing::info!(
        event = "host_place_change_requested",
        "The app was asked to return to its first screen"
    );
    Ok(StatusCode::NO_CONTENT)
}
