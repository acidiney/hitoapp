use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use hito_core::{ArtworkAvailability, ReleaseArtworkFetch, Services};

#[utoipa::path(get, path = "/api/v1/releases/{id}/artwork/availability", operation_id = "release_artwork_availability",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtworkAvailability), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn release_availability(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtworkAvailability>> {
    Ok(Json(
        services
            .catalog
            .release_artwork_availability(&actor, &id)
            .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/releases/{id}/artwork/fetch", operation_id = "fetch_release_artwork",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = ReleaseArtworkFetch), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn fetch_release(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<ReleaseArtworkFetch>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            services
                .catalog
                .enqueue_release_artwork(&actor, &id)
                .await?,
        ),
    ))
}

#[utoipa::path(get, path = "/api/v1/release-artwork-fetches/{id}", operation_id = "release_artwork_fetch",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ReleaseArtworkFetch), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn release_fetch(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ReleaseArtworkFetch>> {
    Ok(Json(
        services.catalog.release_artwork_fetch(&actor, &id).await?,
    ))
}
