use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{
    ArtistAssociation, ArtistAssociationChangeReceipt, ArtistAssociationChanges,
    ArtistProfileAvailability, Services, UndoRecordingArtistChange, UnlinkRecordingArtist,
};
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

#[utoipa::path(get, path = "/api/v1/recordings/{id}/artist-association-changes", operation_id = "artist_association_changes",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistAssociationChanges), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_association_changes", request_id = %Uuid::new_v4()))]
pub async fn changes(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtistAssociationChanges>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_artist_association_changes(&actor, &id)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "returned"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist association changes HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/unlink", operation_id = "unlink_recording_artist",
    params(("id" = String, Path)), request_body = UnlinkRecordingArtist,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistAssociationChangeReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_association_unlink", request_id = %Uuid::new_v4()))]
pub async fn unlink(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<UnlinkRecordingArtist>,
) -> ApiResult<Json<ArtistAssociationChangeReceipt>> {
    let started = Instant::now();
    let result = services
        .catalog
        .unlink_recording_artist(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist association unlink HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/undo", operation_id = "undo_recording_artist_change",
    params(("id" = String, Path)), request_body = UndoRecordingArtistChange,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistAssociationChangeReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_association_undo", request_id = %Uuid::new_v4()))]
pub async fn undo(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<UndoRecordingArtistChange>,
) -> ApiResult<Json<ArtistAssociationChangeReceipt>> {
    let started = Instant::now();
    let result = services
        .catalog
        .undo_recording_artist_change(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist association undo HTTP request completed"
    );
    Ok(Json(result?))
}
