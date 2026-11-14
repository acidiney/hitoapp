use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::QueryParams,
};
use axum::{Json, extract::State};
use hito_core::{Genre, GenreDetail, GenreTracksPage, Services};
use serde::Deserialize;
use utoipa::IntoParams;

#[utoipa::path(get, path = "/api/v1/genres", operation_id = "genres",
    description = "Returns the same bounded genre tiles as Discover, with at most 18 entries. Genre.track_count is a bounded display count, may be nonexact, and is not a pagination total.",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Genre], headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn list(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<Genre>>> {
    Ok(Json(services.catalog.genres(&actor).await?))
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct GenreKeyQuery {
    key: String,
}

#[utoipa::path(get, path = "/api/v1/genres/detail", operation_id = "genre_detail",
    description = "Returns the existing genre detail and bounded previews of at most 24 albums and 100 tracks. The track preview is not the first page of /api/v1/genres/tracks, and track_count may be nonexact. Pass the canonical key from Genre.key; the server applies the existing genre-key normalization.",
    params(GenreKeyQuery), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = GenreDetail, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn detail(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<GenreKeyQuery>,
) -> ApiResult<Json<GenreDetail>> {
    Ok(Json(services.catalog.genre(&actor, &query.key).await?))
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct GenreTracksQuery {
    key: String,
    limit: Option<i64>,
    after: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/genres/tracks", operation_id = "genre_tracks",
    description = "Returns a continuation page in inherited title and recording-ID order. Pass the canonical key from Genre.key; the server applies the existing genre-key normalization. limit defaults to 100 and is clamped to 1 through 200. after is an opaque continuation for the same account and normalized key, not an authorization token. Malformed, stale, or out-of-scope continuations return 422 and require restarting pagination. Pages continue through a fixed catalog; catalog changes can alter later pages.",
    params(GenreTracksQuery), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = GenreTracksPage, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn tracks(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<GenreTracksQuery>,
) -> ApiResult<Json<GenreTracksPage>> {
    Ok(Json(
        services
            .catalog
            .genre_tracks(&actor, &query.key, query.limit, query.after.as_deref())
            .await?,
    ))
}
