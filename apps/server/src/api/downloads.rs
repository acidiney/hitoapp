use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::{JsonBody, QueryParams},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use hito_core::{
    DownloadReceipt, DownloadRequest, DownloadStatus, PlaylistDownloadQuery,
    PlaylistDownloadReceipt, PlaylistDownloadStatus, Services,
};

#[utoipa::path(post, path = "/api/v1/recordings/{id}/download", operation_id = "download_recording",
    params(("id" = String, Path)), request_body = DownloadRequest,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = DownloadReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn create(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<DownloadRequest>,
) -> ApiResult<(StatusCode, Json<DownloadReceipt>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(services.downloads.create(&actor, &id, request).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}/download", operation_id = "recording_download",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = DownloadStatus), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn status(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<DownloadStatus>> {
    Ok(Json(services.downloads.status(&actor, &id).await?))
}

#[utoipa::path(delete, path = "/api/v1/downloads/{intent_id}", operation_id = "remove_download",
    params(("intent_id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn remove(
    State(services): State<Services>,
    Account(actor): Account,
    Path(intent): Path<String>,
) -> ApiResult<StatusCode> {
    services.downloads.remove(&actor, &intent).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/playlists/{id}/download", operation_id = "download_playlist",
    params(("id" = String, Path)), request_body = DownloadRequest,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = PlaylistDownloadReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn create_playlist(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<DownloadRequest>,
) -> ApiResult<(StatusCode, Json<PlaylistDownloadReceipt>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            services
                .downloads
                .create_playlist(&actor, &id, request)
                .await?,
        ),
    ))
}

#[utoipa::path(get, path = "/api/v1/playlists/{id}/download", operation_id = "playlist_download",
    params(("id" = String, Path), PlaylistDownloadQuery), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaylistDownloadStatus), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn playlist_status(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(page): QueryParams<PlaylistDownloadQuery>,
) -> ApiResult<Json<PlaylistDownloadStatus>> {
    Ok(Json(
        services
            .downloads
            .playlist_status(&actor, &id, page)
            .await?,
    ))
}
