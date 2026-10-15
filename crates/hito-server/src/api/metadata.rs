use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::QueryParams,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{MetadataHistoryQuery, RecordingMetadata, ReleaseMetadata, Services};

#[utoipa::path(get, path = "/api/v1/recordings/{id}/metadata", params(
    ("id" = String, Path), ("before" = Option<i64>, Query), ("limit" = Option<i64>, Query),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingMetadata), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn recording_metadata(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(page): QueryParams<MetadataHistoryQuery>,
) -> ApiResult<Json<RecordingMetadata>> {
    Ok(Json(
        services
            .catalog
            .recording_metadata(&actor, &id, page)
            .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/releases/{id}/metadata", params(
    ("id" = String, Path), ("before" = Option<i64>, Query), ("limit" = Option<i64>, Query),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ReleaseMetadata), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn release_metadata(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(page): QueryParams<MetadataHistoryQuery>,
) -> ApiResult<Json<ReleaseMetadata>> {
    Ok(Json(
        services.catalog.release_metadata(&actor, &id, page).await?,
    ))
}
