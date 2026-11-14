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
