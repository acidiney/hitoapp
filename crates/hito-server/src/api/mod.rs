pub mod artist_catalog;
pub mod artist_discography;
pub mod artist_profiles;
pub mod artist_review;
pub mod artwork;
pub mod audiobooks;
pub mod browsing;
pub mod catalog;
pub mod collection;
pub mod downloads;
pub mod identity;
pub mod jobs;
pub mod metadata;
pub mod player;
pub mod playlist_follow;
pub mod plugin_authorization;
pub mod plugins;
pub mod podcasts;
pub mod progress;
pub mod providers;
pub mod source_bindings;
pub mod source_choice;
pub mod source_preparation;
pub mod source_refresh;
pub mod source_resolution;
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
