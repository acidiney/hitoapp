use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use hito_core::{ArtworkAvailability, PlaylistArtworkFetch, Services};

#[utoipa::path(get, path = "/api/v1/playlists/{id}/artwork/availability", operation_id = "playlist_artwork_availability",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtworkAvailability), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn availability(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtworkAvailability>> {
    Ok(Json(
        services
            .catalog
            .playlist_artwork_availability(&actor, &id)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/playlists/{id}/artwork/fetch", operation_id = "fetch_playlist_artwork",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = PlaylistArtworkFetch), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn enqueue(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<PlaylistArtworkFetch>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            services
                .catalog
                .enqueue_playlist_artwork(&actor, &id)
                .await?,
        ),
    ))
}

#[utoipa::path(get, path = "/api/v1/playlist-artwork-fetches/{id}", operation_id = "playlist_artwork_fetch",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaylistArtworkFetch), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn receipt(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<PlaylistArtworkFetch>> {
    Ok(Json(
        services.catalog.playlist_artwork_fetch(&actor, &id).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/playlists/{id}/artwork", operation_id = "playlist_artwork",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, description = "Privately retained cover of this imported playlist",
        content(("image/png"), ("image/jpeg"), ("image/gif"), ("image/webp")),
        headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn image(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    super::artwork::image_response(services.catalog.playlist_artwork(&actor, &id).await?)
}
