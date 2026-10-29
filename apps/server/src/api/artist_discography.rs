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
    ArtistDiscographyAvailability, DiscographyLibrary, ReadArtistDiscography,
    ReadDiscographyLibrary, Services,
};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/{association}/discography", operation_id = "recording_artist_discography",
    params(("id" = String, Path), ("association" = String, Path)),
    request_body = ReadArtistDiscography,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistDiscographyAvailability, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_artist_discography", request_id = %Uuid::new_v4()))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, association)): Path<(String, String)>,
    JsonBody(request): JsonBody<ReadArtistDiscography>,
) -> ApiResult<Json<ArtistDiscographyAvailability>> {
    let started = Instant::now();
    let result = services
        .catalog
        .recording_artist_discography_until(
            &actor,
            &id,
            &association,
            request,
            std::future::pending(),
        )
        .await;
    let (outcome, item_count) = match &result {
        Ok(ArtistDiscographyAvailability::Available { page, .. }) => {
            ("available", page.items.len())
        }
        Ok(ArtistDiscographyAvailability::Disabled) => ("disabled", 0),
        Ok(ArtistDiscographyAvailability::Unsupported) => ("unsupported", 0),
        Ok(ArtistDiscographyAvailability::Unavailable) => ("unavailable", 0),
        Ok(ArtistDiscographyAvailability::Cancelled) => ("cancelled", 0),
        Err(error) => (error.code(), 0),
    };
    tracing::debug!(
        outcome,
        item_count,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording artist discography HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-associations/{association}/discography/library", operation_id = "recording_artist_discography_library",
    params(("id" = String, Path), ("association" = String, Path)),
    request_body = ReadDiscographyLibrary,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = DiscographyLibrary, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_recording_artist_discography_library", request_id = %Uuid::new_v4()))]
pub async fn library(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, association)): Path<(String, String)>,
    JsonBody(request): JsonBody<ReadDiscographyLibrary>,
) -> ApiResult<Json<DiscographyLibrary>> {
    let started = Instant::now();
    let result = services
        .catalog
        .discography_library(&actor, &id, &association, request)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "available")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording artist discography library HTTP request completed"
    );
    Ok(Json(result?))
}
