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
use hito_core::{ArtworkAvailability, ArtworkFetch, CoverArtKind, CoverArtLookup, Error, Services};

#[utoipa::path(get, path = "/api/v1/recordings/{id}/artwork/availability", operation_id = "artwork_availability",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtworkAvailability), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn artwork_availability(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtworkAvailability>> {
    Ok(Json(
        services.catalog.artwork_availability(&actor, &id).await?,
    ))
}

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

pub(super) fn image_response(artwork: Option<hito_core::Artwork>) -> ApiResult<Response> {
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

#[utoipa::path(get, path = "/api/v1/cover-art/{kind}/{mbid}", operation_id = "cover_art",
    params(("kind" = String, Path, description = "release, release-group or artist"), ("mbid" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, description = "Front cover from the Cover Art Archive, or an artist photo from Wikimedia Commons, for this MusicBrainz identifier",
        content(("image/png"), ("image/jpeg")),
        headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 202, description = "The image is queued or being retrieved; ask again shortly", body = ErrorResponse),
    (status = 401, body = ErrorResponse),
    (status = 404, description = "There is no image, or retrieval is switched off", body = ErrorResponse),
    (status = 422, body = ErrorResponse),
    (status = 502, description = "The image source could not be reached; it is asked again later", body = ErrorResponse),
))]
pub async fn cover_art(
    State(services): State<Services>,
    Account(actor): Account,
    Path((kind, mbid)): Path<(String, String)>,
) -> ApiResult<Response> {
    let kind = CoverArtKind::from_path(&kind).ok_or(Error::NotFound)?;
    match services.catalog.cover_art(&actor, kind, &mbid).await? {
        CoverArtLookup::Ready(artwork) => image_response(Some(artwork)),
        CoverArtLookup::Pending => Ok((
            StatusCode::ACCEPTED,
            Json(ErrorResponse {
                error: "The cover is being retrieved".into(),
                code: None,
            }),
        )
            .into_response()),
        CoverArtLookup::Unavailable => {
            Err(Error::Upstream("The Cover Art Archive could not be reached").into())
        }
        CoverArtLookup::Missing | CoverArtLookup::Unknown => Err(Error::NotFound.into()),
    }
}
