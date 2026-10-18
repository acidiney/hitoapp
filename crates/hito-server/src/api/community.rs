use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use hito_core::{
    CommunityCredentialReset, CommunityPreferences, CommunityReportAdmission, Services,
    SuccessfulPlayback, UpdateCommunityPreferences,
};

#[utoipa::path(get, path = "/api/v1/community/preferences", operation_id = "community_preferences",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CommunityPreferences), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn preferences(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<CommunityPreferences>> {
    Ok(Json(services.community.preferences(&actor).await?))
}

#[utoipa::path(put, path = "/api/v1/community/preferences", operation_id = "update_community_preferences",
    request_body = UpdateCommunityPreferences, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CommunityPreferences), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn update_preferences(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<UpdateCommunityPreferences>,
) -> ApiResult<Json<CommunityPreferences>> {
    Ok(Json(
        services
            .community
            .update_preferences(&actor, request)
            .await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/community/credentials", operation_id = "reset_community_credentials",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CommunityCredentialReset), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn reset_credentials(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<CommunityCredentialReset>> {
    Ok(Json(services.community.reset_credentials(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/sources/{id}/successful-playback", operation_id = "successful_playback",
    params(("id" = String, Path)), request_body = SuccessfulPlayback,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CommunityReportAdmission), (status = 202, body = CommunityReportAdmission),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn successful_playback(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<SuccessfulPlayback>,
) -> ApiResult<(StatusCode, Json<CommunityReportAdmission>)> {
    let admission = services
        .community
        .observe_success(&actor, &id, request)
        .await?;
    let status = if admission == CommunityReportAdmission::Queued {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(admission)))
}
