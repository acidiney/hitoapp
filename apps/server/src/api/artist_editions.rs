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
    EditionTrackListAvailability, ReadEditionTrackList, ReadReleaseGroupEditions,
    ReleaseGroupEditionsAvailability, Services,
};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/{association}/release-group-editions", operation_id = "recording_artist_release_group_editions",
    params(("id" = String, Path), ("association" = String, Path)),
    request_body = ReadReleaseGroupEditions,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ReleaseGroupEditionsAvailability, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_artist_release_group_editions", request_id = %Uuid::new_v4()))]
pub async fn list(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, association)): Path<(String, String)>,
    JsonBody(request): JsonBody<ReadReleaseGroupEditions>,
) -> ApiResult<Json<ReleaseGroupEditionsAvailability>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_artist_release_group_editions_until(
            &actor,
            &id,
            &association,
            request,
            std::future::pending(),
        )
        .await;
    let (outcome, item_count) = match &result {
        Ok(ReleaseGroupEditionsAvailability::Available { page, .. }) => {
            ("available", page.items.len())
        }
        Ok(ReleaseGroupEditionsAvailability::Disabled) => ("disabled", 0),
        Ok(ReleaseGroupEditionsAvailability::Unsupported) => ("unsupported", 0),
        Ok(ReleaseGroupEditionsAvailability::Unavailable) => ("unavailable", 0),
        Ok(ReleaseGroupEditionsAvailability::Cancelled) => ("cancelled", 0),
        Err(error) => (error.code(), 0),
    };
    tracing::debug!(
        outcome,
        item_count,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording artist release-group editions HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/{association}/edition-track-list", operation_id = "recording_artist_edition_track_list",
    params(("id" = String, Path), ("association" = String, Path)),
    request_body = ReadEditionTrackList,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = EditionTrackListAvailability, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_artist_edition_track_list", request_id = %Uuid::new_v4()))]
pub async fn track_list(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, association)): Path<(String, String)>,
    JsonBody(request): JsonBody<ReadEditionTrackList>,
) -> ApiResult<Json<EditionTrackListAvailability>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_artist_edition_track_list_until(
            &actor,
            &id,
            &association,
            request,
            std::future::pending(),
        )
        .await;
    let (outcome, medium_count, track_count) = match &result {
        Ok(EditionTrackListAvailability::Available { edition, .. }) => (
            "available",
            edition.media.len(),
            edition
                .media
                .iter()
                .map(|medium| medium.tracks.len())
                .sum::<usize>(),
        ),
        Ok(EditionTrackListAvailability::Disabled) => ("disabled", 0, 0),
        Ok(EditionTrackListAvailability::Unsupported) => ("unsupported", 0, 0),
        Ok(EditionTrackListAvailability::Unavailable) => ("unavailable", 0, 0),
        Ok(EditionTrackListAvailability::Cancelled) => ("cancelled", 0, 0),
        Err(error) => (error.code(), 0, 0),
    };
    tracing::debug!(
        outcome,
        medium_count,
        track_count,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording artist edition track list HTTP request completed"
    );
    Ok(Json(result?))
}
