use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{ChooseSource, Services, SourceChoiceReceipt};

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
