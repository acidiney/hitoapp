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
    Activity, HistoryCleared, HistoryPreference, ImportProviderPlaylist,
    ImportedPlaylistMetadataRefresh, Playlist, PlaylistDetail, PlaylistImportReceipt,
    RefreshImportedPlaylistMetadata, Services, SyncBatch, SyncPage, SyncReceipt,
    UpdateHistoryPreference,
};
use serde::Deserialize;
use utoipa::IntoParams;

#[utoipa::path(get, path = "/api/v1/playlists", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Playlist]), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn playlists(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<Playlist>>> {
    Ok(Json(services.collection.playlists(&actor).await?))
}

#[utoipa::path(get, path = "/api/v1/playlists/{id}", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaylistDetail), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn playlist(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<PlaylistDetail>> {
    Ok(Json(services.collection.playlist(&actor, &id).await?))
}

#[utoipa::path(post, path = "/api/v1/plugins/{id}/playlists/import", operation_id = "import_provider_playlist",
    params(("id" = String, Path)), request_body = ImportProviderPlaylist,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaylistImportReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn import_provider_playlist(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ImportProviderPlaylist>,
) -> ApiResult<Json<PlaylistImportReceipt>> {
    Ok(Json(
        services
            .collection
            .import_provider_playlist(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/playlists/{id}/metadata/refresh", operation_id = "refresh_imported_playlist_metadata",
    params(("id" = String, Path)), request_body = RefreshImportedPlaylistMetadata,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ImportedPlaylistMetadataRefresh), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn refresh_imported_playlist_metadata(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<RefreshImportedPlaylistMetadata>,
) -> ApiResult<Json<ImportedPlaylistMetadataRefresh>> {
    Ok(Json(
        services
            .collection
            .refresh_imported_playlist_metadata(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/sync", request_body = SyncBatch,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SyncReceipt), (status = 401, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn sync(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(batch): JsonBody<SyncBatch>,
) -> ApiResult<Json<SyncReceipt>> {
    Ok(Json(services.collection.apply(&actor, batch).await?))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SyncQuery {
    #[serde(default)]
    cursor: i64,
}

#[utoipa::path(get, path = "/api/v1/sync", params(SyncQuery),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SyncPage), (status = 401, body = ErrorResponse),
    (status = 400, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn sync_events(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<SyncQuery>,
) -> ApiResult<Json<SyncPage>> {
    Ok(Json(
        services.collection.events(&actor, query.cursor).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/activity", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = Activity), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn activity(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Activity>> {
    Ok(Json(services.collection.activity(&actor).await?))
}

#[utoipa::path(get, path = "/api/v1/history/preferences", operation_id = "history_preference",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = HistoryPreference, headers(("Cache-Control" = String, description = "no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn history_preference(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<([(&'static str, &'static str); 1], Json<HistoryPreference>)> {
    Ok((
        NO_STORE,
        Json(services.collection.history_preference(&actor).await?),
    ))
}

#[utoipa::path(put, path = "/api/v1/history/preferences", operation_id = "update_history_preference",
    request_body = UpdateHistoryPreference, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = HistoryPreference, headers(("Cache-Control" = String, description = "no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn update_history_preference(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<UpdateHistoryPreference>,
) -> ApiResult<([(&'static str, &'static str); 1], Json<HistoryPreference>)> {
    Ok((
        NO_STORE,
        Json(
            services
                .collection
                .set_history_enabled(&actor, request.enabled)
                .await?,
        ),
    ))
}

#[utoipa::path(delete, path = "/api/v1/history", operation_id = "delete_history",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = HistoryCleared, headers(("Cache-Control" = String, description = "no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn delete_history(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<(
    StatusCode,
    [(&'static str, &'static str); 1],
    Json<HistoryCleared>,
)> {
    Ok((
        StatusCode::OK,
        NO_STORE,
        Json(services.collection.clear_history(&actor).await?),
    ))
}

const NO_STORE: [(&str, &str); 1] = [("cache-control", "no-store")];
