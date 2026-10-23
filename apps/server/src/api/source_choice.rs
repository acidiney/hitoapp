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
    ChooseSource, ResetSourceChoice, Services, SourceChoice, SourceChoiceReceipt,
    SourceChoiceResetReceipt,
};

#[utoipa::path(get, path = "/api/v1/recordings/{id}/source-choice", operation_id = "source_choice",
    params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SourceChoice), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn source_choice(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<SourceChoice>> {
    Ok(Json(services.catalog.source_choice(&actor, &id).await?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/source-choice", operation_id = "choose_source",
    params(("id" = String, Path)), request_body = ChooseSource,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SourceChoiceReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn choose_source(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ChooseSource>,
) -> ApiResult<Json<SourceChoiceReceipt>> {
    Ok(Json(
        services.catalog.choose_source(&actor, &id, request).await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/source-choice/reset", operation_id = "reset_source_choice",
    params(("id" = String, Path)), request_body = ResetSourceChoice,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SourceChoiceResetReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn reset_source_choice(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ResetSourceChoice>,
) -> ApiResult<Json<SourceChoiceResetReceipt>> {
    Ok(Json(
        services
            .catalog
            .reset_source_choice(&actor, &id, request)
            .await?,
    ))
}
