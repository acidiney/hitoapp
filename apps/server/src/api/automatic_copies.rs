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
    AutomaticCopies, AutomaticCopyBudget, AutomaticCopyPreference, Services, UpcomingRecordings,
};

#[utoipa::path(post, path = "/api/v1/playback/upcoming", operation_id = "announce_upcoming_recordings",
    description = "Name up to 2 recordings this client will play next. Each list replaces the previous list from the same session and client tag. The server may save private automatic copies of them while Keep played tracks is on. Recordings the account cannot open are ignored.",
    request_body = UpcomingRecordings, security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 429, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn announce(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<UpcomingRecordings>,
) -> ApiResult<StatusCode> {
    services
        .downloads
        .announce_upcoming(&actor, request)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/account/automatic-copies", operation_id = "automatic_copies",
    description = "Keep played tracks for this account, with the space its automatic copies use and the budget an administrator set.",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AutomaticCopies), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn status(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<AutomaticCopies>> {
    Ok(Json(services.downloads.automatic_copies(&actor).await?))
}

#[utoipa::path(put, path = "/api/v1/account/automatic-copies", operation_id = "update_automatic_copies",
    description = "Turn Keep played tracks on or off. Turning it off removes this account's automatic copies; downloads stay.",
    request_body = AutomaticCopyPreference, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AutomaticCopies), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn update(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<AutomaticCopyPreference>,
) -> ApiResult<Json<AutomaticCopies>> {
    Ok(Json(
        services
            .downloads
            .set_automatic_copies(&actor, request)
            .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/users/{id}/automatic-copy-budget", operation_id = "automatic_copy_budget",
    description = "The storage budget for one account's automatic copies. Administrators only.",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AutomaticCopyBudget), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn budget(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<AutomaticCopyBudget>> {
    Ok(Json(
        services
            .downloads
            .automatic_copy_budget(&actor, &id)
            .await?,
    ))
}

#[utoipa::path(put, path = "/api/v1/users/{id}/automatic-copy-budget", operation_id = "update_automatic_copy_budget",
    description = "Set the storage budget for one account's automatic copies. Copies over a lower budget are removed by score. Administrators only.",
    params(("id" = String, Path)), request_body = AutomaticCopyBudget,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AutomaticCopyBudget), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn update_budget(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<AutomaticCopyBudget>,
) -> ApiResult<Json<AutomaticCopyBudget>> {
    Ok(Json(
        services
            .downloads
            .set_automatic_copy_budget(&actor, &id, request)
            .await?,
    ))
}
