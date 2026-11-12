use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use hito_core::{
    ProviderCandidate, ReadSelectedRecordingLyrics, RecordingLyricIdentities,
    RecordingLyricIdentity, RecordingLyricReview, RecordingLyricServices,
    RetainRecordingLyricIdentity, SearchRecordingLyrics, SelectedLyricsLookup, Services,
};
use hito_plugin_sdk::SearchPage;
use serde::Deserialize;

#[utoipa::path(get, path = "/api/v1/recordings/{id}/lyrics/services", operation_id = "lyric_identity_services",
    params(("id" = String, Path)), 
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingLyricServices, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn services(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<RecordingLyricServices>> {
    Ok(Json(
        services
            .catalog
            .recording_lyric_services(&actor, &id)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/lyrics/candidates", operation_id = "lyric_identity_candidates",
    params(("id" = String, Path)), request_body = SearchRecordingLyrics,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SearchPage, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn candidates(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<SearchRecordingLyrics>,
) -> ApiResult<Json<SearchPage>> {
    Ok(Json(
        services
            .catalog
            .search_recording_lyrics(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/lyrics/identities/review", operation_id = "lyric_identity_review",
    params(("id" = String, Path)), request_body = ProviderCandidate,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingLyricReview, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn review(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ProviderCandidate>,
) -> ApiResult<Json<RecordingLyricReview>> {
    Ok(Json(
        services
            .catalog
            .review_recording_lyric_identity(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/lyrics/identities", operation_id = "lyric_identity_retain",
    params(("id" = String, Path)), request_body = RetainRecordingLyricIdentity,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingLyricIdentity, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn retain(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<RetainRecordingLyricIdentity>,
) -> ApiResult<Json<RecordingLyricIdentity>> {
    Ok(Json(
        services
            .catalog
            .retain_recording_lyric_identity(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}/lyrics/identities", operation_id = "lyric_identity_identities",
    params(("id" = String, Path)), 
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingLyricIdentities, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn identities(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<RecordingLyricIdentities>> {
    Ok(Json(
        services
            .catalog
            .recording_lyric_identities(&actor, &id)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/lyrics/identities/read", operation_id = "lyric_identity_read",
    params(("id" = String, Path)), request_body = ReadSelectedRecordingLyrics,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SelectedLyricsLookup, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ReadSelectedRecordingLyrics>,
) -> ApiResult<Json<SelectedLyricsLookup>> {
    Ok(Json(
        services
            .catalog
            .selected_recording_lyrics_until(&actor, &id, request, std::future::pending())
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityRevision {
    pub revision: i64,
}

#[utoipa::path(delete, path = "/api/v1/recordings/{id}/lyrics/identities/{identity_id}", operation_id = "lyric_identity_remove",
    params(("id" = String, Path), ("identity_id" = String, Path), ("revision" = i64, Query)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 204, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn remove(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, identity)): Path<(String, String)>,
    Query(request): Query<IdentityRevision>,
) -> ApiResult<StatusCode> {
    services
        .catalog
        .remove_recording_lyric_identity(&actor, &id, &identity, request.revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
