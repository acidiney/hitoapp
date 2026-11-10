use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::{JsonBody, QueryParams},
};
use axum::{
    Json,
    extract::{Path, Request, State},
    response::Response,
};
use hito_core::{AudioStream, Library, Page, Recording, Search, Services, Source};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

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
    ("sort" = Option<hito_core::CatalogSort>, Query),
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

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordingLookupRequest {
    #[schema(max_items = 500)]
    pub recording_ids: Vec<Uuid>,
}

const LOOKUP_LIMIT: usize = 500;

#[utoipa::path(post, path = "/api/v1/recordings/lookup", operation_id = "recordings_lookup",
    request_body = RecordingLookupRequest, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Recording]), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn recordings_lookup(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<RecordingLookupRequest>,
) -> ApiResult<Json<Vec<Recording>>> {
    if request.recording_ids.len() > LOOKUP_LIMIT {
        return Err(hito_core::Error::Invalid(format!(
            "Look up at most {LOOKUP_LIMIT} recordings at a time"
        ))
        .into());
    }
    let started = std::time::Instant::now();
    let found = services
        .catalog
        .readable_recordings(&actor, &request.recording_ids)
        .await?;
    tracing::debug!(
        event = "recordings_lookup_response",
        requested = request.recording_ids.len(),
        returned = found.len(),
        outcome = "read",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording lookup returned"
    );
    Ok(Json(found))
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

#[utoipa::path(get, path = "/api/v1/recordings/{id}/playback-sources", operation_id = "playback_sources",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Source]), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn playback_sources(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<Source>>> {
    Ok(Json(
        services
            .catalog
            .playback_sources_until(&actor, &id, std::future::pending())
            .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/sources/{id}/audio", params(
    ("id" = String, Path), ("Range" = Option<String>, Header),
    crate::audio::AudioQuery,
), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, content((AudioStream = "application/octet-stream"), (AudioStream = "audio/aac")), headers(
        ("X-Hito-Stream-Quality" = hito_core::StreamQuality, description = "Effective representation quality"),
        ("X-Hito-Stream-Start-Ms" = u64, description = "Converted segment start in milliseconds; absent for originals"),
    )),
    (status = 206, body = AudioStream, content_type = "application/octet-stream"),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 416),
    (status = 409, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn audio(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(query): QueryParams<crate::audio::AudioQuery>,
    request: Request,
) -> ApiResult<Response> {
    crate::audio::response(&services.playback, &actor, &id, query, request).await
}

#[utoipa::path(head, path = "/api/v1/sources/{id}/audio", params(
    ("id" = String, Path),
    crate::audio::AudioQuery,
),
    security(("session" = []), ("bearer" = [])), responses(
        (status = 200, description = "Audio representation headers without starting an encoder", headers(
            ("X-Hito-Stream-Quality" = hito_core::StreamQuality, description = "Effective representation quality"),
            ("X-Hito-Stream-Start-Ms" = u64, description = "Converted segment start in milliseconds; absent for originals"),
        )),
        (status = 400, body = ErrorResponse), (status = 422, body = ErrorResponse),
        (status = 401), (status = 403), (status = 404), (status = 409), (status = 502),
))]
pub async fn audio_head(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(query): QueryParams<crate::audio::AudioQuery>,
    request: Request,
) -> ApiResult<Response> {
    crate::audio::response(&services.playback, &actor, &id, query, request).await
}
