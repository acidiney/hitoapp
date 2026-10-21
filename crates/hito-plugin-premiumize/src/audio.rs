use crate::policy::{AuthorizedConnection, Error};
use axum::{
    body::Body,
    extract::{Query, rejection::QueryRejection},
    http::{HeaderMap, Method},
    response::Response,
};
use futures_util::stream::try_unfold;
use hito_debrid_premiumize::OpenedBoundAudio;
use serde::Deserialize;
use std::{io, time::Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioQuery {
    source_id: String,
}

pub(crate) async fn response(
    AuthorizedConnection(connection): AuthorizedConnection,
    method: Method,
    headers: HeaderMap,
    query: Result<Query<AudioQuery>, QueryRejection>,
) -> Result<Response, Error> {
    if method != Method::GET {
        return Err(Error::MethodNotAllowed);
    }
    if headers.contains_key("range") || headers.contains_key("if-range") {
        return Err(Error::InvalidRequest);
    }
    let Query(query) = query.map_err(|_| Error::InvalidRequest)?;
    if query.source_id.trim().is_empty()
        || query.source_id.len() > 256
        || query.source_id.chars().any(char::is_control)
    {
        return Err(Error::InvalidRequest);
    }
    let entry = connection.catalog.source(&query.source_id)?;
    let audio = connection
        .client
        .open_bound_audio(&entry.binding, &connection.media_origins)
        .await
        .map_err(|_| Error::Unavailable)?;
    let length = audio.length;
    let stream = try_unfold(
        Relay {
            audio,
            remaining: length,
            log: AudioLog {
                started: Instant::now(),
                dispatch: tracing::dispatcher::get_default(Clone::clone),
                span: tracing::Span::current(),
                bytes: 0,
                finished: false,
            },
        },
        |mut relay| async move {
            if relay.remaining == 0 {
                return Ok::<_, io::Error>(None);
            }
            let chunk = match relay.audio.next_chunk().await {
                Ok(Some(chunk)) if chunk.len() as u64 <= relay.remaining => chunk,
                _ => {
                    relay.log.finish("failed");
                    return Err(body_error());
                }
            };
            relay.remaining -= chunk.len() as u64;
            if relay.remaining == 0 && !matches!(relay.audio.next_chunk().await, Ok(None)) {
                relay.log.finish("failed");
                return Err(body_error());
            }
            relay.log.bytes += chunk.len() as u64;
            if relay.remaining == 0 {
                relay.log.finish("complete");
            }
            Ok(Some((chunk, relay)))
        },
    );
    Response::builder()
        .header("content-type", "audio/wav")
        .header("content-length", length)
        .body(Body::from_stream(stream))
        .map_err(|_| Error::Unavailable)
}

struct Relay {
    audio: OpenedBoundAudio,
    remaining: u64,
    log: AudioLog,
}

fn body_error() -> io::Error {
    io::Error::other("The curated audio representation failed")
}

struct AudioLog {
    started: Instant,
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    bytes: u64,
    finished: bool,
}

impl AudioLog {
    fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let _entered = self.span.enter();
        tracing::debug!(
            event = "premiumize_plugin_audio_finished",
            outcome,
            bytes = self.bytes,
            elapsed_ms = self.started.elapsed().as_millis() as u64
        );
    }
}

impl Drop for AudioLog {
    fn drop(&mut self) {
        self.finish("cancelled");
    }
}
