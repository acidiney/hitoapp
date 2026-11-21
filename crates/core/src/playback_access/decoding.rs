mod opening;
mod process;
mod profile;

pub use opening::PcmOpening;
use opening::Stop;

use super::{PlaybackSource, conversion::ConversionLimits};
use crate::{AudioFile, Error, MediaTools, Result};
use bytes::Bytes;
use futures_util::{future::BoxFuture, stream::BoxStream};
use std::{
    future::Future,
    io::Seek,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

pub trait PcmInput: Send + Sync + 'static {
    fn correlation_id(&self) -> u64;
    fn check(&self) -> BoxFuture<'_, Result<()>>;
    fn revoked(&self) -> BoxFuture<'_, Error>;
    fn bytes(&self) -> BoxFuture<'_, Result<BoxStream<'static, Result<Bytes>>>>;
}

#[derive(Clone)]
pub struct StreamDecoder {
    context: DecodeContext,
}

impl StreamDecoder {
    pub fn new(tools: MediaTools) -> Self {
        let limits = ConversionLimits::new(tools.parallelism);
        Self {
            context: DecodeContext { tools, limits },
        }
    }
}

#[derive(Clone)]
pub(super) enum Authority {
    Grant(Box<PlaybackSource>),
    Input(Arc<dyn PcmInput>),
}

impl Authority {
    pub(super) fn correlation_id(&self) -> u64 {
        match self {
            Self::Grant(grant) => grant.correlation_id(),
            Self::Input(input) => input.correlation_id(),
        }
    }

    pub(super) async fn check(&self) -> Result<()> {
        match self {
            Self::Grant(grant) => grant.check().await,
            Self::Input(input) => input.check().await,
        }
    }

    pub(super) async fn revoked(&self) -> Error {
        match self {
            Self::Grant(grant) => grant.revoked().await,
            Self::Input(input) => input.revoked().await,
        }
    }
}

pub(super) enum Feed {
    File(AudioFile),
    Stream(Arc<dyn PcmInput>),
}

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
    authority: Authority,
    receiver: mpsc::Receiver<Message>,
    pending: Option<Message>,
    cancellation: Stop,
    owner: Option<JoinHandle<()>>,
    finished: bool,
    authorizing: Option<Pin<Box<dyn Future<Output = Result<()>> + Send>>>,
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
        let authority = Authority::Grant(Box::new(self.clone()));
        let (format, slot) = profile::probe(
            &self.decoding.tools,
            &Feed::File(AudioFile {
                file: file.file.try_clone().map_err(|_| unfinished())?,
                length: file.length,
                content_type: file.content_type,
            }),
            authority.clone(),
            slot,
            &stop,
        )
        .await?;
        stop.check()?;
        file.file.rewind().map_err(|_| unfinished())?;
        let stream = process::start(
            &self.decoding.tools,
            Feed::File(file),
            format,
            start,
            authority,
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

impl StreamDecoder {
    async fn decode(
        &self,
        input: Arc<dyn PcmInput>,
        start: Duration,
        stop: Stop,
    ) -> Result<PcmStream> {
        stop.check()?;
        input.check().await?;
        stop.check()?;
        let slot = self.context.limits.acquire("stream")?;
        let started = Instant::now();
        let authority = Authority::Input(input.clone());
        let (format, slot) = profile::probe(
            &self.context.tools,
            &Feed::Stream(input.clone()),
            authority.clone(),
            slot,
            &stop,
        )
        .await?;
        stop.check()?;
        let stream = process::start(
            &self.context.tools,
            Feed::Stream(input),
            format,
            start,
            authority,
            slot,
            stop,
        )
        .await?;
        tracing::debug!(
            event = "native_pcm_started",
            sample_rate = format.sample_rate,
            channels = format.channels,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Native decoder ready from a stream"
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
        self.authorize().await?;
        if self.pending.is_none() {
            self.pending = Some(tokio::select! {
                biased;
                error = self.authority.revoked() => Message::Failed(error),
                message = self.receiver.recv() => message.unwrap_or_else(|| Message::Failed(unfinished())),
            });
        }
        self.authorize().await?;
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

    async fn authorize(&mut self) -> Result<()> {
        let authority = self.authority.clone();
        let check = self
            .authorizing
            .get_or_insert_with(|| Box::pin(async move { authority.check().await }));
        let result = check.await;
        self.authorizing = None;
        result
    }

    fn cancel(&mut self) {
        self.authorizing = None;
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
