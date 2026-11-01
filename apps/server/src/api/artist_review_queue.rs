use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::{JsonBody, QueryParams},
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{
    ArtistReviewConfirmation, ArtistReviewDismissal, ArtistReviewItem, ArtistReviewQueueQuery,
    ArtistReviewRestoration, ConfirmArtistReviewCandidate, DismissArtistReviewCandidates, Page,
    RestoreArtistReviewCandidates, Services,
};
use serde::Serialize;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
#[serde(transparent)]
pub struct ArtistReviewQueuePage(pub Page<ArtistReviewItem>);

#[utoipa::path(get, path = "/api/v1/artist-matching/review-queue", operation_id = "artist_review_queue",
    params(("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistReviewQueuePage), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_review_queue", request_id = %Uuid::new_v4()))]
pub async fn queue(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<ArtistReviewQueueQuery>,
) -> ApiResult<Json<ArtistReviewQueuePage>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .artist_review_queue(&actor, query)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist review queue HTTP request completed"
    );
    Ok(Json(ArtistReviewQueuePage(result?)))
}

#[utoipa::path(post, path = "/api/v1/artist-matching/review-queue/{id}/confirm", operation_id = "confirm_artist_review_candidate",
    params(("id" = String, Path)), request_body = ConfirmArtistReviewCandidate,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistReviewConfirmation), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_review_confirm", request_id = %Uuid::new_v4()))]
pub async fn confirm(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ConfirmArtistReviewCandidate>,
) -> ApiResult<Json<ArtistReviewConfirmation>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .confirm_artist_review_candidate(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "confirmed")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist review confirmation HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artist-matching/review-queue/{id}/dismiss", operation_id = "dismiss_artist_review_candidates",
    params(("id" = String, Path)), request_body = DismissArtistReviewCandidates,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistReviewDismissal), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_review_dismiss", request_id = %Uuid::new_v4()))]
pub async fn dismiss(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<DismissArtistReviewCandidates>,
) -> ApiResult<Json<ArtistReviewDismissal>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .dismiss_artist_review_candidates(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "dismissed")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist review dismissal HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(get, path = "/api/v1/artist-matching/review-queue/dismissed", operation_id = "dismissed_artist_reviews",
    params(("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistReviewQueuePage), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_review_dismissed", request_id = %Uuid::new_v4()))]
pub async fn dismissed(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<ArtistReviewQueueQuery>,
) -> ApiResult<Json<ArtistReviewQueuePage>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .dismissed_artist_reviews(&actor, query)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Dismissed artist reviews HTTP request completed"
    );
    Ok(Json(ArtistReviewQueuePage(result?)))
}

#[utoipa::path(post, path = "/api/v1/artist-matching/review-queue/{id}/restore", operation_id = "restore_artist_review_candidates",
    params(("id" = String, Path)), request_body = RestoreArtistReviewCandidates,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistReviewRestoration), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_review_restore", request_id = %Uuid::new_v4()))]
pub async fn restore(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<RestoreArtistReviewCandidates>,
) -> ApiResult<Json<ArtistReviewRestoration>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .restore_artist_review_candidates(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "restored")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist review restoration HTTP request completed"
    );
    Ok(Json(result?))
}
