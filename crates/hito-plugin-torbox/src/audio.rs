use crate::{http::HttpState, journal::Target, policy::Error, preparation};
use axum::{
    body::Body,
    extract::{Query, State, rejection::QueryRejection},
    http::Method,
    response::Response,
};
use futures_util::stream::try_unfold;
use hito_debrid_torbox::{Error as ProviderError, OpenedBoundAudio};
use hito_plugin_sdk::PreparationReason;
use serde::Deserialize;
use std::{io, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioQuery {
    source_id: String,
}

pub(crate) async fn response(
    State(state): State<Arc<HttpState>>,
    query: Result<Query<AudioQuery>, QueryRejection>,
    method: Method,
) -> Result<Response, Error> {
    let Query(query) = query.map_err(|_| Error::InvalidRequest)?;
    if query.source_id.trim().is_empty()
        || query.source_id.len() > 256
        || query.source_id.chars().any(char::is_control)
    {
        return Err(Error::InvalidRequest);
    }
    let index = state.catalog.source(&query.source_id)?;
    let (length, body) = if method == Method::HEAD {
        if !preparation::ready(&state, index).await? {
            return Err(Error::Unavailable);
        }
        (state.catalog.entry(index).binding.file_bytes, Body::empty())
    } else {
        let opening = state
            .client
            .admit_bound_audio()
            .map_err(|_| Error::Unavailable)?;
        let target = Target::from(state.catalog.entry(index));
        let snapshot = state.journal.snapshot(target.clone()).await?;
        let audio = match opening
            .open_retained(&snapshot.value, &state.media_origins)
            .await
        {
            Ok(audio) => audio,
            Err(error) => {
                if !matches!(
                    error,
                    ProviderError::Busy | ProviderError::Status(429 | 500..=599)
                ) {
                    let reason = if error == ProviderError::SourceChanged {
                        PreparationReason::SourceChanged
                    } else {
                        PreparationReason::SourceUnavailable
                    };
                    state
                        .journal
                        .invalidate(target.clone(), snapshot.operation, reason)
                        .await?;
                }
                return Err(Error::Unavailable);
            }
        };
        if state.journal.candidate(target).await? != Some(snapshot.operation) {
            return Err(Error::Unavailable);
        }
        let length = audio.length;
        let stream = try_unfold(
            Relay {
                audio,
                remaining: length,
            },
            |mut relay| async move {
                if relay.remaining == 0 {
                    return Ok::<_, io::Error>(None);
                }
                let chunk = relay
                    .audio
                    .next_chunk()
                    .await
                    .map_err(|_| body_error())?
                    .ok_or_else(body_error)?;
                relay.remaining = relay
                    .remaining
                    .checked_sub(chunk.len() as u64)
                    .ok_or_else(body_error)?;
                if relay.remaining == 0 && !matches!(relay.audio.next_chunk().await, Ok(None)) {
                    return Err(body_error());
                }
                Ok(Some((chunk, relay)))
            },
        );
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
}
fn body_error() -> io::Error {
    io::Error::other("The curated audio representation failed")
}
