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
    ArtistMatchReceipt, ArtistMatchSuggestions, ArtistMatchingStatus, ArtistProviderMatchReceipt,
    DismissArtistMatchSuggestion, MatchArtistsFromProviderEvidence, MatchArtistsFromTags,
    RestoreArtistMatchSuggestion, Services,
};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/artist-matching", operation_id = "match_artists_from_tags",
    request_body = MatchArtistsFromTags, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistMatchReceipt), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_matching", request_id = %Uuid::new_v4()))]
pub async fn run(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<MatchArtistsFromTags>,
) -> ApiResult<Json<ArtistMatchReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .match_artists_from_tags(&actor, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist matching HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artist-matching/provider-evidence", operation_id = "match_artists_from_provider_evidence",
    request_body = MatchArtistsFromProviderEvidence, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistProviderMatchReceipt), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_matching_provider", request_id = %Uuid::new_v4()))]
pub async fn run_provider_evidence(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<MatchArtistsFromProviderEvidence>,
) -> ApiResult<Json<ArtistProviderMatchReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .match_artists_from_provider_evidence(&actor, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist provider matching HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(get, path = "/api/v1/artists/{id}/match-suggestions", operation_id = "artist_match_suggestions",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistMatchSuggestions), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_match_suggestions", request_id = %Uuid::new_v4()))]
pub async fn suggestions(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtistMatchSuggestions>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .artist_match_suggestions(&actor, &id)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist match suggestions HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artists/{id}/match-suggestions/dismiss", operation_id = "dismiss_artist_match_suggestion",
    params(("id" = String, Path)), request_body = DismissArtistMatchSuggestion,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistMatchSuggestions), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_match_dismiss", request_id = %Uuid::new_v4()))]
pub async fn dismiss(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<DismissArtistMatchSuggestion>,
) -> ApiResult<Json<ArtistMatchSuggestions>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .dismiss_artist_match_suggestion(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "dismissed")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist match dismissal HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artists/{id}/match-suggestions/restore", operation_id = "restore_artist_match_suggestion",
    params(("id" = String, Path)), request_body = RestoreArtistMatchSuggestion,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistMatchSuggestions), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_match_restore", request_id = %Uuid::new_v4()))]
pub async fn restore(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<RestoreArtistMatchSuggestion>,
) -> ApiResult<Json<ArtistMatchSuggestions>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .restore_artist_match_suggestion(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "restored")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist match restore HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(get, path = "/api/v1/artist-matching/status", operation_id = "artist_matching_status",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistMatchingStatus), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn status(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<ArtistMatchingStatus>> {
    Ok(Json(
        services
            .artist_catalog
            .artist_matching_status(&actor)
            .await?,
    ))
}
