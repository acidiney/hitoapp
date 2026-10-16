pub mod artwork;
pub mod browsing;
pub mod catalog;
pub mod collection;
pub mod identity;
pub mod jobs;
pub mod metadata;
pub mod player;
pub mod plugin_authorization;
pub mod plugins;
pub mod providers;
pub mod source_refresh;
pub mod storage;

use axum::{Extension, Json, extract::State, http::StatusCode};
use hito_core::Services;
use serde::Serialize;
use std::sync::Arc;
use utoipa::{ToSchema, openapi::OpenApi};

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub ready: bool,
}

#[utoipa::path(get, path = "/health", responses(
    (status = 200, body = Health), (status = 503, body = Health),
))]
pub async fn health(State(services): State<Services>) -> (StatusCode, Json<Health>) {
    let ready = services.healthy().await;
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(Health { ready }))
}

#[utoipa::path(get, path = "/api/v1/openapi.json", responses(
    (status = 200, body = Object, content_type = "application/json"),
))]
pub async fn contract(Extension(document): Extension<Arc<OpenApi>>) -> Json<OpenApi> {
    Json((*document).clone())
}
