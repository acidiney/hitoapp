use crate::{
    auth::{Account, LockableAccount},
    error::{ApiResult, ErrorResponse},
    extract::{JsonBody, QueryParams},
};
use axum::{
    Json,
    extract::{Path, Request, State, rejection::PathRejection},
    http::StatusCode,
    response::Response,
};
use hito_core::{
    AudioStream, BeginPlaybackLease, Error, PlaybackLease, ReleaseClientLeases, Services,
};
use uuid::Uuid;

#[utoipa::path(post, path = "/api/v1/playback/leases", operation_id = "playback_lease_begin",
    description = "Retain this foreground session's admitted recording and source for original audio playback. The expiry is an original Unix timestamp in seconds and is not renewed by reading the lease. The handle is not authorization and does not grant downloads or offline access. A client may tag its leases with a stable `client` it keeps across restarts, and may send a `request_id`: repeating a request from the same session returns the lease it already began instead of a new one.",
    request_body = BeginPlaybackLease,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaybackLease), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_playback_lease_begin")
)]
pub async fn begin(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<BeginPlaybackLease>,
) -> ApiResult<Json<PlaybackLease>> {
    Ok(Json(services.playback.begin_lease(&actor, request).await?))
}

#[utoipa::path(delete, path = "/api/v1/playback/leases", operation_id = "playback_lease_release_client",
    description = "Retire every lease this session began with the given client tag and join their work. A client calls this when it starts, to return the leases an earlier run left behind. Leases of other sessions and other tags are untouched.",
    params(ReleaseClientLeases), security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_playback_lease_release_client")
)]
pub async fn release_client(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
    QueryParams(query): QueryParams<ReleaseClientLeases>,
) -> ApiResult<StatusCode> {
    services
        .playback
        .release_client_leases(&actor, &query.client)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/playback/leases/{id}", operation_id = "playback_lease_status",
    description = "Read the lease while the exact owning session and retained source grant remain authorized. Status does not renew its expiry.",
    params(("id" = Uuid, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlaybackLease), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_playback_lease_status")
)]
pub async fn status(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
    path: Result<Path<String>, PathRejection>,
) -> ApiResult<Json<PlaybackLease>> {
    Ok(Json(
        services
            .playback
            .lease_status(&actor, parse_path(path)?)
            .await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/playback/leases/{id}", operation_id = "playback_lease_release",
    description = "Retire the owning session's lease and join its work. Owned terminal and unknown handles are idempotent; known foreign-session handles are unavailable.",
    params(("id" = Uuid, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_playback_lease_release")
)]
pub async fn release(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
    path: Result<Path<String>, PathRejection>,
) -> ApiResult<StatusCode> {
    services
        .playback
        .release_lease(&actor, parse_path(path)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/playback/leases/{id}/audio", operation_id = "playback_lease_audio",
    description = "Stream the admitted audio. The default original representation supports single byte ranges when the source is seekable; If-Range ignores Range because this route supplies no conditional validator. High and saver qualities stream AAC from start_ms with no Content-Length or byte ranges, and Range is ignored. When conversion is unavailable, including remote-only sources, the response is 409 and the client should retry the original.",
    params(("id" = Uuid, Path), ("Range" = Option<String>, Header), ("If-Range" = Option<String>, Header), crate::audio::AudioQuery),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, content((AudioStream = "application/octet-stream"), (AudioStream = "audio/aac")), headers(
        ("X-Hito-Stream-Quality" = hito_core::StreamQuality, description = "Effective representation quality"),
        ("X-Hito-Stream-Start-Ms" = u64, description = "Converted segment start in milliseconds; absent for originals"),
    )),
    (status = 206, body = AudioStream, content_type = "application/octet-stream"),
    (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 416), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_playback_lease_audio", account_id = actor.user().id.as_str()))]
pub async fn audio(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
    path: Result<Path<String>, PathRejection>,
    QueryParams(query): QueryParams<crate::audio::AudioQuery>,
    request: Request,
) -> ApiResult<Response> {
    crate::audio::lease_response(
        &services.playback,
        &actor,
        parse_path(path)?,
        query,
        request,
    )
    .await
}

#[utoipa::path(head, path = "/api/v1/playback/leases/{id}/audio", operation_id = "playback_lease_audio_head",
    description = "Read the representation headers without an audio body or starting an encoder. Range is ignored for HEAD. For high or saver quality, the headers report the representation GET would stream; when conversion is unavailable they describe the original.",
    params(("id" = Uuid, Path), crate::audio::AudioQuery), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, headers(
        ("X-Hito-Stream-Quality" = hito_core::StreamQuality, description = "Effective representation quality"),
        ("X-Hito-Stream-Start-Ms" = u64, description = "Converted segment start in milliseconds; absent for originals"),
    )), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_playback_lease_audio_head", account_id = actor.user().id.as_str()))]
pub async fn audio_head(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
    path: Result<Path<String>, PathRejection>,
    QueryParams(query): QueryParams<crate::audio::AudioQuery>,
    request: Request,
) -> ApiResult<Response> {
    crate::audio::lease_response(
        &services.playback,
        &actor,
        parse_path(path)?,
        query,
        request,
    )
    .await
}

fn parse_path(path: Result<Path<String>, PathRejection>) -> hito_core::Result<Uuid> {
    let invalid =
        || Error::Invalid("Playback lease path does not match the expected fields".into());
    let Path(id) = path.map_err(|_| invalid())?;
    Uuid::parse_str(&id).map_err(|_| invalid())
}
