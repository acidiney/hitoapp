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
    BrowseDirectories, ConnectFolder, DirectoryPage, Job, MissingRecordingsRemoval, MusicFolder,
    Services, SharedDirectory,
};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderRevision {
    pub revision: i64,
}

#[utoipa::path(get, path = "/api/v1/storage/directories", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [SharedDirectory]), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn directories(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<SharedDirectory>>> {
    Ok(Json(services.storage.shared_directories(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/storage/browse", request_body = BrowseDirectories,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = DirectoryPage), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn browse(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<BrowseDirectories>,
) -> ApiResult<Json<DirectoryPage>> {
    Ok(Json(services.storage.browse(&actor, request).await?))
}

#[utoipa::path(get, path = "/api/v1/libraries/{library}/folders", params(("library" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [MusicFolder]), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn folders(
    State(services): State<Services>,
    Account(actor): Account,
    Path(library): Path<String>,
) -> ApiResult<Json<Vec<MusicFolder>>> {
    Ok(Json(services.storage.folders(&actor, &library).await?))
}

#[utoipa::path(post, path = "/api/v1/libraries/{library}/folders", params(("library" = String, Path)), request_body = ConnectFolder,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 201, body = MusicFolder), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn connect(
    State(services): State<Services>,
    Account(actor): Account,
    Path(library): Path<String>,
    JsonBody(request): JsonBody<ConnectFolder>,
) -> ApiResult<(StatusCode, Json<MusicFolder>)> {
    Ok((
        StatusCode::CREATED,
        Json(
            services
                .storage
                .connect_folder(&actor, &library, request)
                .await?,
        ),
    ))
}

#[utoipa::path(delete, path = "/api/v1/folders/{id}", params(("id" = String, Path)), request_body = FolderRevision,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
))]
pub async fn remove(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<FolderRevision>,
) -> ApiResult<StatusCode> {
    services
        .storage
        .remove_folder(&actor, &id, request.revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/folders/{id}/scan", operation_id = "scan_folder", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = Job), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
))]
pub async fn scan(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(services.storage.enqueue_folder_scan(&actor, &id).await?),
    ))
}

#[utoipa::path(delete, path = "/api/v1/libraries/{library}/missing-recordings", params(("library" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = MissingRecordingsRemoval), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn remove_missing_recordings(
    State(services): State<Services>,
    Account(actor): Account,
    Path(library): Path<String>,
) -> ApiResult<Json<MissingRecordingsRemoval>> {
    Ok(Json(
        services
            .storage
            .remove_missing_recordings(&actor, &library)
            .await?,
    ))
}
