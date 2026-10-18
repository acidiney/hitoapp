use super::{Pcm16WavFormat, wav};
use crate::{Error, TorrentFileBinding};
use bytes::Bytes;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tokio::sync::OwnedSemaphorePermit;

const MAX_CHUNK_BYTES: usize = 65_536;
const BODY_IDLE: Duration = Duration::from_secs(15);

pub struct OpenedBoundAudio {
    pub binding: TorrentFileBinding,
    pub length: u64,
    pub format: Pcm16WavFormat,
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
}

impl OpenedBoundAudio {
    pub(crate) async fn qualify(
        mut response: reqwest::Response,
        binding: TorrentFileBinding,
        length: u64,
        permit: OwnedSemaphorePermit,
    ) -> Result<Self, Error> {
        let mut prefix = [0_u8; 44];
        let mut filled = 0;
        let mut pending = VecDeque::new();
        let mut received = 0_u64;
        while filled < prefix.len() {
            let chunk = read_chunk(&mut response).await?.ok_or(Error::MediaLength)?;
            received = received
                .checked_add(chunk.len() as u64)
                .ok_or(Error::MediaLength)?;
            if received > length {
                return Err(Error::MediaLength);
            }
            let needed = (prefix.len() - filled).min(chunk.len());
            prefix[filled..filled + needed].copy_from_slice(&chunk[..needed]);
            filled += needed;
            pending.push_back(chunk);
        }
        let format = wav::qualify(&prefix, length)?;
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
        })
    }

    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>, Error> {
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
                    return Err(Error::MediaLength);
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
                Err(Error::MediaLength)
            }
            Err(error) => {
                self.finish(error.outcome());
                Err(error)
            }
        }
    }

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
                event = "real_debrid_audio_body_finished",
                outcome,
                bytes = self.delivered,
                elapsed_ms = self.started.elapsed().as_millis() as u64,
                "Real-Debrid bound audio body finished"
            );
        });
    }
}

impl Drop for OpenedBoundAudio {
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
