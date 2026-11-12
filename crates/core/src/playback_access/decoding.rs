mod opening;
mod process;
mod profile;

pub use opening::PcmOpening;
use opening::Stop;

use super::{PlaybackSource, conversion::ConversionLimits};
use crate::{Error, MediaTools, Result};
use std::{
    io::Seek,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone)]
pub(super) struct DecodeContext {
    pub tools: MediaTools,
    pub limits: ConversionLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration: Option<Duration>,
}

pub struct PcmBlock {
    pub samples: Vec<f32>,
    pub position: Duration,
}

enum Message {
    Block(PcmBlock),
    End,
    Failed(Error),
}

pub struct PcmStream {
    format: PcmFormat,
    authority: PlaybackSource,
    receiver: mpsc::Receiver<Message>,
    pending: Option<Message>,
    cancellation: Stop,
    owner: Option<JoinHandle<()>>,
    finished: bool,
}

impl PlaybackSource {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playback_grant = self.correlation_id(), account_id = self.actor().id()))]
    pub async fn open_pcm(&self, start: Duration) -> Result<PcmStream> {
        self.begin_pcm(start)?.ready().await
    }

    async fn decode_pcm(&self, start: Duration, stop: Stop) -> Result<PcmStream> {
        stop.check()?;
        self.check().await?;
        stop.check()?;
        let slot = self.decoding.limits.acquire(self.actor().id())?;
        let started = Instant::now();
        let mut file = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(opening::cancelled()),
            file = self.open_file_if_present() => file?.ok_or(Error::UnsupportedAudio)?,
        };
        let (format, slot) =
            profile::probe(&self.decoding.tools, &file, self.clone(), slot, &stop).await?;
        stop.check()?;
        file.file.rewind().map_err(|_| unfinished())?;
        let stream = process::start(
            &self.decoding.tools,
            file,
            format,
            start,
            self.clone(),
            slot,
            stop,
        )
        .await?;
        tracing::debug!(
            event = "native_pcm_started",
            sample_rate = format.sample_rate,
            channels = format.channels,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Native decoder ready"
        );
        Ok(stream)
    }
}

impl PcmStream {
    pub fn format(&self) -> PcmFormat {
        self.format
    }

    #[tracing::instrument(level = "trace", skip_all, fields(playback_grant = self.authority.correlation_id()))]
    pub async fn next_block(&mut self) -> Result<Option<PcmBlock>> {
        if self.finished {
            return Ok(None);
        }
        self.authority.check().await?;
        if self.pending.is_none() {
            self.pending = Some(tokio::select! {
                biased;
                error = self.authority.revoked() => Message::Failed(error),
                message = self.receiver.recv() => message.unwrap_or_else(|| Message::Failed(unfinished())),
            });
        }
        self.authority.check().await?;
        match self.pending.take().ok_or_else(unfinished)? {
            Message::Block(block) => Ok(Some(block)),
            terminal => {
                self.finished = true;
                self.receiver.close();
                match terminal {
                    Message::End => Ok(None),
                    Message::Failed(error) => Err(error),
                    Message::Block(_) => unreachable!(),
                }
            }
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(playback_grant = self.authority.correlation_id()))]
    pub async fn close(mut self) -> Result<()> {
        self.cancel();
        if let Some(owner) = self.owner.take() {
            owner.await.map_err(|_| unfinished())?;
        }
        Ok(())
    }

    fn cancel(&mut self) {
        self.pending = None;
        self.receiver.close();
        self.cancellation.cancel();
    }
}

impl Drop for PcmStream {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn unfinished() -> Error {
    Error::Upstream("Native audio decoding could not finish")
}
