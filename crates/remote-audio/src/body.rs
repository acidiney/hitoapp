use crate::{AudioFormat, AudioKind, Error, format};
use bytes::Bytes;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tokio::sync::OwnedSemaphorePermit;

const MAX_CHUNK_BYTES: usize = 65_536;
const BODY_IDLE: Duration = Duration::from_secs(15);

pub struct OpenedBoundAudio<B, E = Error> {
    pub binding: B,
    pub length: u64,
    pub format: AudioFormat,
    response: Option<reqwest::Response>,
    pending: VecDeque<Bytes>,
    permit: Option<OwnedSemaphorePermit>,
    received: u64,
    delivered: u64,
    expected_length: u64,
    started: Instant,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
    finished: bool,
    error: std::marker::PhantomData<fn() -> E>,
}

impl<B, E: From<Error>> OpenedBoundAudio<B, E> {
    pub(crate) async fn qualify(
        mut response: reqwest::Response,
        binding: B,
        length: u64,
        kind: AudioKind,
        permit: OwnedSemaphorePermit,
    ) -> Result<Self, Error> {
        let mut prefix = Vec::new();
        let prefix_limit = kind.prefix_limit();
        let mut pending = VecDeque::new();
        let mut received = 0_u64;
        let format = loop {
            if let Some(format) = format::qualify(kind, &prefix, length)? {
                break format;
            }
            if prefix.len() == prefix_limit {
                return Err(Error::UnsupportedAudio);
            }
            let chunk = read_chunk(&mut response).await?.ok_or(Error::MediaLength)?;
            received = received
                .checked_add(chunk.len() as u64)
                .ok_or(Error::MediaLength)?;
            if received > length {
                return Err(Error::MediaLength);
            }
            let needed = (prefix_limit - prefix.len()).min(chunk.len());
            prefix.extend_from_slice(&chunk[..needed]);
            pending.push_back(chunk);
        };
        Ok(Self {
            binding,
            length,
            format,
            response: Some(response),
            pending,
            permit: Some(permit),
            received,
            delivered: 0,
            expected_length: length,
            started: Instant::now(),
            span: tracing::Span::current(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            finished: false,
            error: std::marker::PhantomData,
        })
    }

    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>, E> {
        if self.finished {
            return Ok(None);
        }
        if let Some(mut chunk) = self.pending.pop_front() {
            let output = chunk.split_to(chunk.len().min(MAX_CHUNK_BYTES));
            if !chunk.is_empty() {
                self.pending.push_front(chunk);
            }
            self.delivered += output.len() as u64;
            return Ok(Some(output));
        }
        let result = match self.response.as_mut() {
            Some(response) => read_chunk(response).await,
            None => Err(Error::MediaTransport),
        };
        match result {
            Ok(Some(mut chunk)) => {
                let received = self.received.checked_add(chunk.len() as u64);
                if received.is_none_or(|received| received > self.expected_length) {
                    self.finish("media_length");
                    return Err(Error::MediaLength.into());
                }
                self.received = received.ok_or(Error::MediaLength)?;
                let output = chunk.split_to(chunk.len().min(MAX_CHUNK_BYTES));
                if !chunk.is_empty() {
                    self.pending.push_back(chunk);
                }
                self.delivered += output.len() as u64;
                Ok(Some(output))
            }
            Ok(None) if self.received == self.expected_length => {
                self.finish("complete");
                Ok(None)
            }
            Ok(None) => {
                self.finish("media_length");
                Err(Error::MediaLength.into())
            }
            Err(error) => {
                self.finish(error.outcome());
                Err(error.into())
            }
        }
    }
}

impl<B, E> OpenedBoundAudio<B, E> {
    fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.response.take();
        self.pending.clear();
        self.permit.take();
        tracing::dispatcher::with_default(&self.dispatch, || {
            let _entered = self.span.enter();
            tracing::debug!(
                event = "remote_audio_body_finished",
                outcome,
                bytes = self.delivered,
                elapsed_ms = self.started.elapsed().as_millis() as u64,
                "Remote audio body finished"
            );
        });
    }
}

impl<B, E> Drop for OpenedBoundAudio<B, E> {
    fn drop(&mut self) {
        self.finish("cancelled");
    }
}

async fn read_chunk(response: &mut reqwest::Response) -> Result<Option<Bytes>, Error> {
    tokio::time::timeout(BODY_IDLE, async {
        for _ in 0..16 {
            match response.chunk().await.map_err(|_| Error::MediaTransport)? {
                Some(chunk) if chunk.is_empty() => continue,
                chunk => return Ok(chunk),
            }
        }
        Err(Error::MediaResponse)
    })
    .await
    .map_err(|_| Error::MediaIdle)?
}
