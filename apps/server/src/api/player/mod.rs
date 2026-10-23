pub(crate) mod audiobooks;
mod error;
pub(crate) mod observations;
mod wire;
use crate::{auth::Account, error::ErrorResponse, extract::JsonBody};
use axum::{Extension, Json};
use error::Error;
use hito_playback::{Player, PlayerAttachment, PlayerCommand, PlayerQueuePage, PlayerReceipt};
use wire::{PlayerClaim, PlayerQueueRequest, PlayerStatus};

#[derive(Clone)]
pub(crate) struct Playback {
    player: Option<Player>,
    streams: std::sync::Arc<tokio::sync::Semaphore>,
}
impl Playback {
    pub(crate) fn new(player: Option<Player>) -> Self {
        Self {
            player,
            streams: std::sync::Arc::new(tokio::sync::Semaphore::new(32)),
        }
    }
    pub(crate) fn native_enabled(&self) -> bool {
        self.player.as_ref().is_some_and(Player::native_enabled)
    }
    fn player(&self) -> Result<&Player, Error> {
        self.player
            .as_ref()
            .ok_or(hito_playback::PlayerError::Unsupported.into())
    }
}

#[utoipa::path(get, path = "/api/v1/player", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlayerStatus), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 429, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn inspect(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
) -> Result<Json<PlayerStatus>, Error> {
    let state = match playback.player {
        Some(player) => {
            let current = player.inspect(&actor).await?;
            PlayerStatus {
                native: current.native,
                epoch: Some(current.epoch),
                state: current.state,
            }
        }
        None => PlayerStatus {
            native: false,
            epoch: None,
            state: None,
        },
    };
    Ok(Json(state))
}

#[utoipa::path(post, path = "/api/v1/player/claim", request_body = PlayerClaim,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlayerAttachment), (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 429, body = ErrorResponse), (status = 501, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn claim(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
    JsonBody(claim): JsonBody<PlayerClaim>,
) -> Result<Json<PlayerAttachment>, Error> {
    Ok(Json(playback.player()?.claim(&actor, claim.epoch).await?))
}

#[utoipa::path(post, path = "/api/v1/player/commands", request_body = PlayerCommand,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlayerReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 429, body = ErrorResponse), (status = 501, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn submit(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
    JsonBody(command): JsonBody<PlayerCommand>,
) -> Result<Json<PlayerReceipt>, Error> {
    Ok(Json(playback.player()?.submit(&actor, command).await?))
}

#[utoipa::path(post, path = "/api/v1/player/queue", request_body = PlayerQueueRequest,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlayerQueuePage), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 429, body = ErrorResponse), (status = 501, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn queue(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
    JsonBody(query): JsonBody<PlayerQueueRequest>,
) -> Result<Json<PlayerQueuePage>, Error> {
    Ok(Json(
        playback
            .player()?
            .queue(
                &actor,
                query.lease,
                query.revision,
                query.offset,
                query.limit,
            )
            .await?,
    ))
}
