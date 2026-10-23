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
    FollowProviderPlaylist, PlaylistFollow, PlaylistFollowReceipt, PlaylistFollowRefresh,
    RefreshFollowedPlaylist, Services,
};

#[utoipa::path(post, path = "/api/v1/plugins/{id}/playlists/follow", operation_id = "follow_provider_playlist",
    params(("id" = String, Path)), request_body = FollowProviderPlaylist,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaylistFollowReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn follow_provider_playlist(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<FollowProviderPlaylist>,
) -> ApiResult<Json<PlaylistFollowReceipt>> {
    Ok(Json(
        services
            .collection
            .follow_provider_playlist(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/playlists/{id}/follow", operation_id = "playlist_follow",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaylistFollow), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn playlist_follow(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<PlaylistFollow>> {
    Ok(Json(
        services.collection.playlist_follow(&actor, &id).await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/playlists/{id}/follow/refresh", operation_id = "refresh_followed_playlist",
    params(("id" = String, Path)), request_body = RefreshFollowedPlaylist,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = PlaylistFollowRefresh), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn refresh_followed_playlist(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<RefreshFollowedPlaylist>,
) -> ApiResult<(StatusCode, Json<PlaylistFollowRefresh>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            services
                .collection
                .enqueue_follow_refresh(&actor, &id, request)
                .await?,
        ),
    ))
}
