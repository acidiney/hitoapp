use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{ArtistAssociation, ArtistProfileAvailability, Services};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(get, path = "/api/v1/recordings/{id}/artist-associations", operation_id = "artist_associations",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [ArtistAssociation]), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_associations", request_id = %Uuid::new_v4()))]
pub async fn associations(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ArtistAssociation>>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_artist_associations(&actor, &id)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        association_count = result.as_ref().map(|items| items.len()).unwrap_or(0),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist associations HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}/artist-associations/{association}/profile", operation_id = "recording_artist_profile",
    params(("id" = String, Path), ("association" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistProfileAvailability), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_artist_profile", request_id = %Uuid::new_v4()))]
pub async fn profile(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, association)): Path<(String, String)>,
) -> ApiResult<Json<ArtistProfileAvailability>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_artist_profile_until(&actor, &id, &association, std::future::pending())
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording artist profile HTTP request completed"
    );
    Ok(Json(result?))
}
