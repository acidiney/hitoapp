use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{Json, extract::State};
use hito_core::{RecordingProgress, Services};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedPlaybackRequest {
    #[schema(max_items = 200)]
    pub recording_ids: Vec<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/recordings/progress", operation_id = "saved_playback",
    request_body = SavedPlaybackRequest, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [RecordingProgress]), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn saved_playback(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<SavedPlaybackRequest>,
) -> ApiResult<Json<Vec<RecordingProgress>>> {
    let started = std::time::Instant::now();
    let progress = services
        .collection
        .saved_playback(&actor, &request.recording_ids)
        .await?;
    tracing::debug!(
        event = "saved_playback_response",
        requested = request.recording_ids.len(),
        returned = progress.len(),
        outcome = "read",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Saved playback returned"
    );
    Ok(Json(progress))
}
