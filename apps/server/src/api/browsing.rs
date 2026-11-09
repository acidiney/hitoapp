use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::QueryParams,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{Artist, ArtistDetail, Browse, Page, Release, ReleaseDetail, Services};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
#[serde(transparent)]
pub struct ReleasePage(pub Page<Release>);

#[derive(Serialize, ToSchema)]
#[serde(transparent)]
pub struct ArtistPage(pub Page<Artist>);

#[utoipa::path(get, path = "/api/v1/releases", params(
    ("q" = Option<String>, Query), ("library" = Option<String>, Query), ("artist" = Option<String>, Query),
    ("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query), ("sort" = Option<hito_core::CatalogSort>, Query),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ReleasePage), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn releases(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(browse): QueryParams<Browse>,
) -> ApiResult<Json<ReleasePage>> {
    Ok(Json(ReleasePage(
        services.catalog.releases(&actor, browse).await?,
    )))
}

#[utoipa::path(get, path = "/api/v1/releases/{id}", params(
    ("id" = String, Path), ("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ReleaseDetail), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn release(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(browse): QueryParams<Browse>,
) -> ApiResult<Json<ReleaseDetail>> {
    Ok(Json(
        services.artist_catalog.release(&actor, &id, browse).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/artists", params(
    ("q" = Option<String>, Query), ("library" = Option<String>, Query),
    ("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query), ("sort" = Option<hito_core::CatalogSort>, Query),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistPage), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn artists(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(browse): QueryParams<Browse>,
) -> ApiResult<Json<ArtistPage>> {
    Ok(Json(ArtistPage(
        services.catalog.artists(&actor, browse).await?,
    )))
}

#[utoipa::path(get, path = "/api/v1/artists/{id}", params(
    ("id" = String, Path), ("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query),
    ("recording_offset" = Option<i64>, Query), ("featured_offset" = Option<i64>, Query),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistDetail), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn artist(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(browse): QueryParams<Browse>,
) -> ApiResult<Json<ArtistDetail>> {
    Ok(Json(services.catalog.artist(&actor, &id, browse).await?))
}
