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
    AttachReviewedProviderSource, ProviderCandidate, ProviderSourceReview, Recording, Services,
};

#[utoipa::path(post, path = "/api/v1/recordings/{id}/sources/review", operation_id = "review_provider_source",
    params(("id" = String, Path)), request_body = ProviderCandidate,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ProviderSourceReview), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn review(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(candidate): JsonBody<ProviderCandidate>,
) -> ApiResult<Json<ProviderSourceReview>> {
    Ok(Json(
        services
            .catalog
            .review_provider_source(&actor, &id, candidate)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/sources/attach", operation_id = "attach_provider_source",
    params(("id" = String, Path)), request_body = AttachReviewedProviderSource,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = Recording), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn attach(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<AttachReviewedProviderSource>,
) -> ApiResult<Json<Recording>> {
    Ok(Json(
        services
            .catalog
            .attach_provider_source(&actor, &id, request)
            .await?,
    ))
}
