use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::{JsonBody, QueryParams},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use hito_core::{
    AudiobookChapterAnchor, AudiobookDetail, AudiobookLibraryPage, AudiobookLibraryQuery,
    AudiobookPlayback, Error, Playlist, Services,
};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

const MAX_CHAPTERS: usize = 1000;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AudiobookChapterCheck {
    #[schema(max_items = 1000)]
    pub chapters: Vec<AudiobookChapterAnchor>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AudiobookAdmissionRequest {
    pub target: AudiobookChapterAnchor,
    #[schema(min_items = 1, max_items = 1000)]
    pub chapters: Vec<AudiobookChapterAnchor>,
}

#[utoipa::path(get, path = "/api/v1/audiobooks", operation_id = "audiobooks",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Playlist], headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn list(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<Playlist>>> {
    Ok(Json(services.collection.audiobooks(&actor).await?))
}

#[utoipa::path(get, path = "/api/v1/audiobooks/{id}", operation_id = "audiobook",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AudiobookDetail, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn detail(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<AudiobookDetail>> {
    Ok(Json(services.collection.audiobook(&actor, &id).await?))
}

#[utoipa::path(get, path = "/api/v1/audiobooks/{id}/playback", operation_id = "audiobook_playback",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AudiobookPlayback, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
pub async fn playback(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<AudiobookPlayback>> {
    Ok(Json(
        services.collection.audiobook_playback(&actor, &id).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/audiobooks/page", operation_id = "audiobook_library",
    params(AudiobookLibraryQuery),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = AudiobookLibraryPage, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
pub async fn page(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<AudiobookLibraryQuery>,
) -> ApiResult<Json<AudiobookLibraryPage>> {
    Ok(Json(
        services.collection.audiobook_library(&actor, query).await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/audiobooks/chapters/check", operation_id = "audiobook_chapters_check",
    description = "Confirm that every chapter still belongs to one of this account's books and is still in a library it can play. A player checks a long selection in chunks.",
    request_body = AudiobookChapterCheck, security(("session" = []), ("bearer" = [])), responses(
    (status = 204, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_audiobook_chapters_check")
)]
pub async fn check_chapters(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<AudiobookChapterCheck>,
) -> ApiResult<StatusCode> {
    services
        .collection
        .check_audiobook_chapters(&actor, &request.chapters)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/audiobooks/{id}/admission", operation_id = "audiobook_admission",
    description = "Admit playback of the target chapter of this account's book. The target must be one of the chapters, every chapter must belong to the book, and the target must have playable audio.",
    params(("id" = Uuid, Path)), request_body = AudiobookAdmissionRequest,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 204, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(operation = "http_audiobook_admission")
)]
pub async fn admission(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<AudiobookAdmissionRequest>,
) -> ApiResult<StatusCode> {
    if Uuid::parse_str(&id).ok() != Some(request.target.audiobook_id)
        || request.chapters.len() > MAX_CHAPTERS
    {
        return Err(Error::Invalid("The audiobook playback selection is invalid".into()).into());
    }
    services
        .collection
        .admit_audiobook_playback(&actor, request.target, &request.chapters)
        .await?
        .publish(|| ())?;
    Ok(StatusCode::NO_CONTENT)
}
