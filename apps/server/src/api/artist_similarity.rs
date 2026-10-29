use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{ReadSimilarArtists, Services, SimilarArtistsAvailability};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/{association}/similar-artists", operation_id = "recording_similar_artists",
    params(("id" = String, Path), ("association" = String, Path)),
    request_body = ReadSimilarArtists,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SimilarArtistsAvailability, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_similar_artists", request_id = %Uuid::new_v4()))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, association)): Path<(String, String)>,
    JsonBody(request): JsonBody<ReadSimilarArtists>,
) -> ApiResult<Json<SimilarArtistsAvailability>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_similar_artists_until(&actor, &id, &association, request, std::future::pending())
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|found| match found {
                SimilarArtistsAvailability::Available { .. } => "available",
                SimilarArtistsAvailability::Disabled => "disabled",
                SimilarArtistsAvailability::Unsupported => "unsupported",
                SimilarArtistsAvailability::Unavailable => "unavailable",
                SimilarArtistsAvailability::Cancelled => "cancelled",
            })
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording similar artists HTTP request completed"
    );
    Ok(Json(result?))
}
