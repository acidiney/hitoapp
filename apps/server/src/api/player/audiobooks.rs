use super::{
    Playback,
    error::Error,
    wire::{PlayerAudiobookCancel, PlayerAudiobookSettlement},
};
use crate::{auth::Account, error::ErrorResponse, extract::JsonBody};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, http::StatusCode};
use hito_playback::{PlayerAudiobookContinuation, PlayerAudiobookRequest};

#[utoipa::path(post, path = "/api/v1/player/audiobooks/prepare", operation_id = "prepare_player_audiobook",
    request_body = PlayerAudiobookRequest, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlayerAudiobookContinuation, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse),
    (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 429, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 501, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn prepare(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
    JsonBody(input): JsonBody<PlayerAudiobookRequest>,
) -> Result<Json<PlayerAudiobookContinuation>, Error> {
    Ok(Json(
        playback.player()?.prepare_audiobook(&actor, input).await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/player/audiobooks/cancel", operation_id = "cancel_player_audiobook",
    request_body = PlayerAudiobookCancel, security(("session" = []), ("bearer" = [])), responses(
    (status = 204, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 200, body = PlayerAudiobookSettlement, headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 429, body = ErrorResponse),
    (status = 501, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn cancel(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
    JsonBody(input): JsonBody<PlayerAudiobookCancel>,
) -> Result<Response, Error> {
    if let Some(command) = input.command {
        let receipt = playback
            .player()?
            .settle_audiobook(&actor, input.lease, input.id, command)
            .await?;
        return Ok(Json(PlayerAudiobookSettlement { receipt }).into_response());
    }
    playback
        .player()?
        .cancel_audiobook(&actor, input.lease, input.id)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
