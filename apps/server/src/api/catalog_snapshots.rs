use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
    http::StatusCode,
};
use hito_core::{BeginCatalogSnapshot, CatalogSnapshot, CatalogSnapshotPage, Error, Services};
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/catalog/snapshots", operation_id = "catalog_snapshot_begin",
    description = "Begin or resume an immutable snapshot of local recording and source facts. Snapshot-only metadata does not advance collection synchronization.",
    request_body = BeginCatalogSnapshot,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CatalogSnapshot), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_catalog_snapshot_begin")
)]
pub async fn begin(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<BeginCatalogSnapshot>,
) -> ApiResult<Json<CatalogSnapshot>> {
    Ok(Json(
        services
            .catalog_replication
            .begin(&actor, &request.library_id)
            .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/catalog/snapshots/{id}/pages/{ordinal}", operation_id = "catalog_snapshot_page",
    description = "Read an immutable snapshot page while its lifetime and current library authority remain valid.",
    params(("id" = Uuid, Path), ("ordinal" = u64, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CatalogSnapshotPage), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_catalog_snapshot_page")
)]
pub async fn page(
    State(services): State<Services>,
    Account(actor): Account,
    path: Result<Path<(String, String)>, PathRejection>,
) -> ApiResult<Json<CatalogSnapshotPage>> {
    let Path((id, ordinal)) = path.map_err(|_| invalid_path())?;
    let id = parse_id(&id)?;
    let ordinal = ordinal.parse::<u64>().map_err(|_| invalid_path())?;
    Ok(Json(
        services
            .catalog_replication
            .page(&actor, id, ordinal)
            .await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/catalog/snapshots/{id}", operation_id = "catalog_snapshot_release",
    description = "Release this account's snapshot. Unknown, expired and foreign handles are idempotent; current library membership is not required for owned cleanup.",
    params(("id" = Uuid, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_catalog_snapshot_release")
)]
pub async fn release(
    State(services): State<Services>,
    Account(actor): Account,
    path: Result<Path<String>, PathRejection>,
) -> ApiResult<StatusCode> {
    let Path(id) = path.map_err(|_| invalid_path())?;
    services
        .catalog_replication
        .release(&actor, parse_id(&id)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_id(id: &str) -> hito_core::Result<Uuid> {
    Uuid::parse_str(id).map_err(|_| invalid_path())
}

fn invalid_path() -> Error {
    Error::Invalid("Catalog snapshot path does not match the expected fields".into())
}
