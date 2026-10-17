use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hito_core::{ArtworkFetch, Error, Services};

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artwork/fetch", operation_id = "fetch_recording_artwork",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = ArtworkFetch), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn fetch_recording_artwork(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<ArtworkFetch>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            services
                .catalog
                .enqueue_recording_artwork(&actor, &id)
                .await?,
        ),
    ))
}

#[utoipa::path(get, path = "/api/v1/artwork-fetches/{id}", operation_id = "artwork_fetch",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtworkFetch), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn artwork_fetch(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtworkFetch>> {
    Ok(Json(services.catalog.artwork_fetch(&actor, &id).await?))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}/artwork", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, description = "Embedded front cover or neighboring raster cover file",
        content(("image/png"), ("image/jpeg"), ("image/gif"), ("image/webp")),
        headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn artwork(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let artwork = services.catalog.artwork(&actor, &id).await?;
    image_response(artwork)
}

#[utoipa::path(get, path = "/api/v1/releases/{id}/artwork", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, description = "Cover from an authorized source on this release",
        content(("image/png"), ("image/jpeg"), ("image/gif"), ("image/webp")),
        headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn release_artwork(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    image_response(services.catalog.release_artwork(&actor, &id).await?)
}

fn image_response(artwork: Option<hito_core::Artwork>) -> ApiResult<Response> {
    let artwork = artwork.ok_or(Error::NotFound)?;
    Ok((
        [
            ("content-type", artwork.content_type),
            ("cache-control", "private, no-store"),
        ],
        artwork.bytes,
    )
        .into_response())
}
