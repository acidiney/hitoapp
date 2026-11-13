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
    ArtistAssociation, ArtistCandidatePreviewAvailability, ArtistCandidateReleasesAvailability,
    ArtistCandidateSearchAvailability, ArtistReviewContext, CreditMemberReviewContext, Error,
    MetadataProvider, Services,
};
use hito_plugin_sdk::ArtistSearchRequest;
use serde::Deserialize;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BeginArtistReview {
    pub provider: MetadataProvider,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArtistCandidates {
    pub context: ArtistReviewContext,
    pub search: ArtistSearchRequest,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectArtistCandidate {
    pub context: ArtistReviewContext,
    pub provider_artist_id: String,
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-review/context", operation_id = "begin_artist_review",
    params(("id" = String, Path)), request_body = BeginArtistReview,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistReviewContext), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_review_context", request_id = %Uuid::new_v4()))]
pub async fn context(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<BeginArtistReview>,
) -> ApiResult<Json<ArtistReviewContext>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .begin_artist_review(&actor, &id, request.provider)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist review context HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/credit-members/{artist}/artist-review/context", operation_id = "begin_credit_member_review",
    params(("id" = String, Path), ("artist" = String, Path)), request_body = BeginArtistReview,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CreditMemberReviewContext), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_credit_member_review_context", request_id = %Uuid::new_v4()))]
pub async fn member_context(
    State(services): State<Services>,
    Account(actor): Account,
    Path((recording, artist)): Path<(String, String)>,
    JsonBody(request): JsonBody<BeginArtistReview>,
) -> ApiResult<Json<CreditMemberReviewContext>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .begin_credit_member_review(&actor, &recording, &artist, request.provider)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "returned"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Credit member review context HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-review/search", operation_id = "search_artist_candidates",
    params(("id" = String, Path)), request_body = SearchArtistCandidates,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistCandidateSearchAvailability), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_candidate_search", request_id = %Uuid::new_v4()))]
pub async fn search(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<SearchArtistCandidates>,
) -> ApiResult<Json<ArtistCandidateSearchAvailability>> {
    let started = Instant::now();
    let result = async {
        require_recording(&id, &request.context)?;
        services
            .artist_catalog
            .search_artist_candidates_until(
                &actor,
                &request.context,
                request.search,
                std::future::pending(),
            )
            .await
    }
    .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist candidate search HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-review/preview", operation_id = "preview_artist_candidate",
    params(("id" = String, Path)), request_body = SelectArtistCandidate,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistCandidatePreviewAvailability), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_candidate_preview", request_id = %Uuid::new_v4()))]
pub async fn preview(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<SelectArtistCandidate>,
) -> ApiResult<Json<ArtistCandidatePreviewAvailability>> {
    let started = Instant::now();
    let result = async {
        require_recording(&id, &request.context)?;
        services
            .artist_catalog
            .preview_artist_candidate_until(
                &actor,
                &request.context,
                &request.provider_artist_id,
                std::future::pending(),
            )
            .await
    }
    .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist candidate preview HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-review/releases", operation_id = "list_artist_candidate_releases",
    params(("id" = String, Path)), request_body = SelectArtistCandidate,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistCandidateReleasesAvailability), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_candidate_releases", request_id = %Uuid::new_v4()))]
pub async fn releases(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<SelectArtistCandidate>,
) -> ApiResult<Json<ArtistCandidateReleasesAvailability>> {
    let started = Instant::now();
    let result = async {
        require_recording(&id, &request.context)?;
        services
            .artist_catalog
            .known_releases_of_artist_candidate_until(
                &actor,
                &request.context,
                &request.provider_artist_id,
                std::future::pending(),
            )
            .await
    }
    .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist candidate releases HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-review/confirm", operation_id = "confirm_artist_review",
    params(("id" = String, Path)), request_body = SelectArtistCandidate,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistAssociation), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_review_confirm", request_id = %Uuid::new_v4()))]
pub async fn confirm(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<SelectArtistCandidate>,
) -> ApiResult<Json<ArtistAssociation>> {
    let started = Instant::now();
    let result = async {
        require_recording(&id, &request.context)?;
        services
            .artist_catalog
            .confirm_artist_review(&actor, &request.context, &request.provider_artist_id)
            .await
    }
    .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist review confirmation HTTP request completed"
    );
    Ok(Json(result?))
}

fn require_recording(recording: &str, context: &ArtistReviewContext) -> hito_core::Result<()> {
    if recording != context.recording_id {
        return Err(Error::Invalid(
            "Use the recording that belongs to this artist review".into(),
        ));
    }
    Ok(())
}
