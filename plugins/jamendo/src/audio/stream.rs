use axum::body::Bytes;
use futures_util::{Stream, stream::try_unfold};
use std::{
    io,
    time::{Duration, Instant},
};
use tokio::sync::OwnedSemaphorePermit;

const MAX_OUTPUT_BYTES: usize = 65_536;

struct Audio {
    response: reqwest::Response,
    pending: Bytes,
    remaining: u64,
    delivered: u64,
    _permit: OwnedSemaphorePermit,
    started: Instant,
    outcome: &'static str,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
}

pub(super) fn audio(
    response: reqwest::Response,
    length: u64,
    permit: OwnedSemaphorePermit,
) -> impl Stream<Item = io::Result<Bytes>> + Send {
    let state = Audio {
        response,
        pending: Bytes::new(),
        remaining: length,
        delivered: 0,
        _permit: permit,
        started: Instant::now(),
        outcome: "canceled",
        span: tracing::Span::current(),
        dispatch: tracing::dispatcher::get_default(Clone::clone),
    };
    try_unfold(state, Audio::next)
}

impl Audio {
    async fn next(mut self) -> io::Result<Option<(Bytes, Self)>> {
        if !self.pending.is_empty() {
            return Ok(Some(self.take_piece()));
        }
        let mut empty_chunks = 0;
        loop {
            let read = tokio::time::timeout(Duration::from_secs(15), self.response.chunk()).await;
            match read {
                Ok(Ok(Some(bytes)))
                    if !bytes.is_empty() && bytes.len() as u64 <= self.remaining =>
                {
                    self.pending = bytes;
                    return Ok(Some(self.take_piece()));
                }
                Ok(Ok(None)) if self.remaining == 0 => {
                    self.outcome = "complete";
                    return Ok(None);
                }
                Ok(Ok(Some(bytes))) if bytes.is_empty() && empty_chunks < 16 => {
                    empty_chunks += 1;
                }
                result => {
                    self.outcome = if result.is_err() {
                        "deadline"
                    } else {
                        "invalid_response"
                    };
                    return Err(io::Error::other("The Jamendo audio representation failed"));
                }
            }
        }
    }

    fn take_piece(mut self) -> (Bytes, Self) {
        let bytes = self
            .pending
            .split_to(self.pending.len().min(MAX_OUTPUT_BYTES));
        self.remaining -= bytes.len() as u64;
        self.delivered += bytes.len() as u64;
        if self.remaining == 0 {
            self.outcome = "complete";
        }
        (bytes, self)
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        tracing::dispatcher::with_default(&self.dispatch, || {
            let _span = self.span.enter();
            tracing::debug!(
                event = "jamendo_audio_complete",
                outcome = self.outcome,
                bytes = self.delivered,
                elapsed_ms = self.started.elapsed().as_millis() as u64
            );
        });
    }
}
