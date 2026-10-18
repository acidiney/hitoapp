use crate::{error::Error, http::HttpState};
use axum::{
    body::Body,
    extract::{Query, State, rejection::QueryRejection},
    http::Method,
    response::Response,
};
use futures_util::stream::try_unfold;
use hito_debrid_real_debrid::{OpenedBoundAudio, TorrentPhase};
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
    let Query(request) = query.map_err(|_| Error::InvalidRequest)?;
    if request.source_id.trim().is_empty()
        || request.source_id.len() > 256
        || request.source_id.chars().any(char::is_control)
    {
        return Err(Error::InvalidRequest);
    }
    let entry = state.catalog.source(&request.source_id)?;
    let (length, body) = if method == Method::HEAD {
        let observed = state
            .client
            .observe(&entry.binding)
            .await
            .map_err(Error::from)?;
        if observed.phase != TorrentPhase::ProviderCompleted {
            return Err(Error::SourceUnavailable);
        }
        (entry.binding.file_bytes, Body::empty())
    } else {
        let opened = state
            .client
            .open_bound_audio(&entry.binding, &state.media_origins)
            .await
            .map_err(Error::from)?;
        let length = opened.length;
        (length, Body::from_stream(stream(opened)))
    };
    Response::builder()
        .header("content-type", "audio/wav")
        .header("content-length", length)
        .body(body)
        .map_err(|_| Error::ProviderUnavailable)
}

fn stream(
    audio: OpenedBoundAudio,
) -> impl futures_util::Stream<Item = io::Result<axum::body::Bytes>> + Send {
    let remaining = audio.length;
    try_unfold(
        (audio, remaining),
        |(mut audio, mut remaining)| async move {
            if remaining == 0 {
                return Ok(None);
            }
            let chunk = audio
                .next_chunk()
                .await
                .map_err(|_| body_error())?
                .ok_or_else(body_error)?;
            remaining = remaining
                .checked_sub(chunk.len() as u64)
                .ok_or_else(body_error)?;
            if remaining == 0
                && audio
                    .next_chunk()
                    .await
                    .map_err(|_| body_error())?
                    .is_some()
            {
                return Err(body_error());
            }
            Ok(Some((chunk, (audio, remaining))))
        },
    )
}

fn body_error() -> io::Error {
    io::Error::other("The curated audio representation failed")
}
