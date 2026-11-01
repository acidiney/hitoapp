use super::artwork::image_response;
use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::QueryParams,
};
use axum::{
    Json,
    extract::{Path, State},
    response::Response,
};
use hito_core::{FeaturedShelf, Services};
use serde::Deserialize;
use utoipa::IntoParams;

#[utoipa::path(get, path = "/api/v1/discover/featured", operation_id = "discover_featured",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = FeaturedShelf, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn shelf(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<FeaturedShelf>> {
    Ok(Json(services.catalog.featured(&actor).await?))
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct FeaturedArtworkQuery {
    revision: i64,
    artwork: String,
}

#[utoipa::path(get, path = "/api/v1/plugins/{id}/featured-artwork", operation_id = "featured_artwork",
    params(("id" = String, Path), FeaturedArtworkQuery),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, description = "Cover of a featured item from the approved connection",
        content(("image/png"), ("image/jpeg")),
        headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn artwork(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(query): QueryParams<FeaturedArtworkQuery>,
) -> ApiResult<Response> {
    let cover = services
        .catalog
        .featured_artwork(&actor, &id, query.revision, &query.artwork)
        .await?;
    image_response(Some(cover))
}
