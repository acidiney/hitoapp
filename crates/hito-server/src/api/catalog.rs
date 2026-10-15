use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::QueryParams,
};
use axum::{
    Json,
    extract::{Path, Request, State},
    response::Response,
};
use hito_core::{AudioStream, Library, Page, Recording, Search, Services, Source};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
#[serde(transparent)]
pub struct RecordingPage(pub Page<Recording>);

#[utoipa::path(get, path = "/api/v1/libraries", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Library]), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn libraries(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<Library>>> {
    Ok(Json(services.catalog.libraries(&actor).await?))
}

#[utoipa::path(get, path = "/api/v1/recordings", params(
    ("q" = Option<String>, Query), ("liked" = Option<bool>, Query), ("artist" = Option<String>, Query),
    ("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query), ("library" = Option<String>, Query),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingPage), (status = 401, body = ErrorResponse),
    (status = 400, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn recordings(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(search): QueryParams<Search>,
) -> ApiResult<Json<RecordingPage>> {
    Ok(Json(RecordingPage(
        services.catalog.recordings(&actor, search).await?,
    )))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}/sources", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Source]), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn sources(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<Source>>> {
    Ok(Json(services.catalog.sources(&actor, &id).await?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/sources/refresh", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Source]), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn refresh_sources(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<Source>>> {
    Ok(Json(services.catalog.refresh_sources(&actor, &id).await?))
}

#[utoipa::path(get, path = "/api/v1/sources/{id}/audio", params(
    ("id" = String, Path), ("Range" = Option<String>, Header),
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AudioStream, content_type = "application/octet-stream"),
    (status = 206, body = AudioStream, content_type = "application/octet-stream"),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 416),
    (status = 409, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn audio(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    request: Request,
) -> ApiResult<Response> {
    crate::audio::response(&services.catalog, &actor, &id, request).await
}

#[utoipa::path(head, path = "/api/v1/sources/{id}/audio", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
        (status = 200, description = "Audio representation headers without a body"),
        (status = 401), (status = 403), (status = 404), (status = 409), (status = 502),
))]
pub async fn audio_head(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    request: Request,
) -> ApiResult<Response> {
    crate::audio::response(&services.catalog, &actor, &id, request).await
}
