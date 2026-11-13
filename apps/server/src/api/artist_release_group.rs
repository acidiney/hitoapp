use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{ReadReleaseGroup, ReleaseGroupAvailability, Services};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/{association}/release-group", operation_id = "recording_artist_release_group",
    params(("id" = String, Path), ("association" = String, Path)),
    request_body = ReadReleaseGroup,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ReleaseGroupAvailability, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_artist_release_group", request_id = %Uuid::new_v4()))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, association)): Path<(String, String)>,
    JsonBody(request): JsonBody<ReadReleaseGroup>,
) -> ApiResult<Json<ReleaseGroupAvailability>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .recording_artist_release_group_until(
            &actor,
            &id,
            &association,
            request,
            std::future::pending(),
        )
        .await;
    tracing::debug!(
        outcome = match &result {
            Ok(ReleaseGroupAvailability::Available { .. }) => "available",
            Ok(ReleaseGroupAvailability::Disabled) => "disabled",
            Ok(ReleaseGroupAvailability::Unsupported) => "unsupported",
            Ok(ReleaseGroupAvailability::Unavailable) => "unavailable",
            Ok(ReleaseGroupAvailability::Cancelled) => "cancelled",
            Err(error) => error.code(),
        },
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording artist release-group HTTP request completed"
    );
    Ok(Json(result?))
}
