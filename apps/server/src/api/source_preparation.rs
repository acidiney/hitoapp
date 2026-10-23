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
use hito_core::{PrepareSource, Services, SourcePreparationReceipt, SourcePreparationStatus};

#[utoipa::path(post, path = "/api/v1/recordings/{recording}/sources/{source}/preparation",
    operation_id = "prepare_source",
    params(("recording" = String, Path), ("source" = String, Path)), request_body = PrepareSource,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = SourcePreparationReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn create(
    State(services): State<Services>,
    Account(actor): Account,
    Path((recording, source)): Path<(String, String)>,
    JsonBody(request): JsonBody<PrepareSource>,
) -> ApiResult<(StatusCode, Json<SourcePreparationReceipt>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            services
                .catalog
                .prepare_source(&actor, &recording, &source, request)
                .await?,
        ),
    ))
}

#[utoipa::path(get, path = "/api/v1/recordings/{recording}/sources/{source}/preparation",
    operation_id = "source_preparation_status",
    params(("recording" = String, Path), ("source" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SourcePreparationStatus), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn status(
    State(services): State<Services>,
    Account(actor): Account,
    Path((recording, source)): Path<(String, String)>,
) -> ApiResult<Json<SourcePreparationStatus>> {
    Ok(Json(
        services
            .catalog
            .source_preparation(&actor, &recording, &source)
            .await?,
    ))
}
