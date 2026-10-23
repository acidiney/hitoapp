use crate::{
    http::{Error, StateData},
    preparation,
};
use axum::{
    body::Body,
    extract::{Query, State, rejection::QueryRejection},
    http::Method,
    response::Response,
};
use futures_util::stream::try_unfold;
use hito_debrid_all_debrid::{MagnetPhase, OpenedBoundAudio};
use serde::Deserialize;
use std::{io, sync::Arc, time::Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioQuery {
    source_id: String,
}

pub(crate) async fn response(
    State(state): State<Arc<StateData>>,
    query: Result<Query<AudioQuery>, QueryRejection>,
    method: Method,
) -> Result<Response, Error> {
    let Query(request) = query.map_err(|_| Error::InvalidRequest)?;
    if request.source_id.trim().is_empty()
        || request.source_id.len() > 256
        || request.source_id.chars().any(char::is_control)
    {
        return Err(Error::InvalidRequest);
    }
    let entry = state.catalog.source(&request.source_id)?;
    let (length, body) = if method == Method::HEAD {
        if !preparation::has_candidate(&entry)? {
            return Err(Error::Unavailable);
        }
        let observed = state
            .client
            .observe(&entry.configured.binding)
            .await
            .map_err(|_| Error::Unavailable)?;
        if observed.phase != MagnetPhase::ProviderCompleted || !preparation::has_candidate(&entry)?
        {
            return Err(Error::Unavailable);
        }
        (entry.configured.binding.file_bytes, Body::empty())
    } else {
        let (candidate, lease) = preparation::take_candidate(entry)?;
        let mut log = Completion {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::Span::current(),
            outcome: "cancelled",
            bytes: 0,
        };
        let audio = match state
            .client
            .open_bound_audio(candidate, &state.media_origins)
            .await
        {
            Ok(audio) => audio,
            Err(_) => {
                log.outcome = "unavailable";
                return Err(Error::Unavailable);
            }
        };
        let length = audio.length;
        let relay = Relay {
            audio,
            remaining: length,
            log,
            _lease: lease,
        };
        let stream = try_unfold(relay, |mut relay| async move {
            if relay.remaining == 0 {
                return Ok::<_, io::Error>(None);
            }
            let chunk = match relay.audio.next_chunk().await {
                Ok(Some(chunk)) => chunk,
                _ => {
                    relay.log.outcome = "invalid";
                    drop(relay);
                    return Err(body_error());
                }
            };
            relay.remaining = match relay.remaining.checked_sub(chunk.len() as u64) {
                Some(remaining) => remaining,
                None => {
                    relay.log.outcome = "invalid";
                    drop(relay);
                    return Err(body_error());
                }
            };
            relay.log.bytes += chunk.len() as u64;
            if relay.remaining == 0 {
                if !matches!(relay.audio.next_chunk().await, Ok(None)) {
                    relay.log.outcome = "invalid";
                    drop(relay);
                    return Err(body_error());
                }
                relay.log.outcome = "complete";
            }
            Ok(Some((chunk, relay)))
        });
        (length, Body::from_stream(stream))
    };
    Response::builder()
        .header("content-type", "audio/wav")
        .header("content-length", length)
        .body(body)
        .map_err(|_| Error::Unavailable)
}

struct Relay {
    audio: OpenedBoundAudio,
    remaining: u64,
    log: Completion,
    _lease: preparation::CandidateLease,
}
struct Completion {
    started: Instant,
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    outcome: &'static str,
    bytes: u64,
}
impl Drop for Completion {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            event = "all_debrid_plugin_audio_finished",
            outcome = self.outcome,
            bytes = self.bytes,
            elapsed_ms = self.started.elapsed().as_millis() as u64
        );
    }
}
fn body_error() -> io::Error {
    io::Error::other("The curated audio representation failed")
}
