use super::{Playback, error::Error};
use crate::{auth::Account, error::ErrorResponse, extract::JsonBody};
use axum::{
    Extension,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::{Stream, stream};
use hito_playback::{PlayerError, PlayerPosition, PlayerSnapshot};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, watch};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayerObservation {
    lease: Uuid,
}

#[utoipa::path(post, path = "/api/v1/player/state/events", request_body = PlayerObservation,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlayerSnapshot, content_type = "text/event-stream", description = "Each SSE data frame contains a PlayerSnapshot; the stream ends when its lease closes"),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 429, body = ErrorResponse), (status = 501, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn state(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
    JsonBody(input): JsonBody<PlayerObservation>,
) -> Result<Response, Error> {
    let permit = playback
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| PlayerError::Busy)?;
    let observed = playback.player()?.subscribe(&actor, input.lease).await?;
    Ok(response(observed, permit, "state", &actor.user().id))
}

#[utoipa::path(post, path = "/api/v1/player/position/events", request_body = PlayerObservation,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PlayerPosition, content_type = "text/event-stream", description = "Each SSE data frame contains a PlayerPosition; the stream ends when its lease closes"),
    (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 429, body = ErrorResponse), (status = 501, body = ErrorResponse), (status = 503, body = ErrorResponse),
))]
pub async fn position(
    Account(actor): Account,
    Extension(playback): Extension<Playback>,
    JsonBody(input): JsonBody<PlayerObservation>,
) -> Result<Response, Error> {
    let permit = playback
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| PlayerError::Busy)?;
    let observed = playback
        .player()?
        .subscribe_position(&actor, input.lease)
        .await?;
    Ok(response(observed, permit, "position", &actor.user().id))
}

struct Subscription {
    _permit: OwnedSemaphorePermit,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        tracing::dispatcher::with_default(&self.dispatch, || {
            self.span.in_scope(|| {
                tracing::debug!(
                    event = "player_observation_closed",
                    "Playback observation closed"
                );
            })
        });
    }
}

fn response<T: Clone + Serialize + Send + Sync + 'static>(
    observed: watch::Receiver<T>,
    permit: OwnedSemaphorePermit,
    kind: &'static str,
    account: &str,
) -> Response {
    let span = tracing::debug_span!("player_observation", observation_id = %Uuid::new_v4(), account_id = account, kind);
    span.in_scope(|| {
        tracing::debug!(
            event = "player_observation_opened",
            "Playback observation opened"
        )
    });
    let subscription = Subscription {
        _permit: permit,
        span,
        dispatch: tracing::dispatcher::get_default(Clone::clone),
    };
    let mut response = Sse::new(watch(observed, subscription))
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    response
}

fn watch<T: Clone + Serialize + Send + Sync + 'static>(
    observed: watch::Receiver<T>,
    subscription: Subscription,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    stream::unfold(
        (observed, subscription, true),
        |(mut observed, subscription, first)| async move {
            if !first && observed.changed().await.is_err() {
                return None;
            }
            if observed.has_changed().is_err() {
                return None;
            }
            let value = observed.borrow_and_update().clone();
            match Event::default().json_data(value) {
                Ok(event) => Some((Ok(event), (observed, subscription, false))),
                Err(_) => {
                    tracing::error!(
                        event = "player_observation_failed",
                        "Playback observation could not be encoded"
                    );
                    None
                }
            }
        },
    )
}
