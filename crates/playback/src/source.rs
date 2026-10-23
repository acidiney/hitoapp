use crate::SourceStop;
use bytes::Bytes;
use hito_core::{AudioRequest, AudioStream, Error, OpenedAudio, PlaybackSource, Result};
use std::io::{self, Read};
use tokio::runtime::Handle;

pub struct Source {
    pub(crate) grant: PlaybackSource,
    pub(crate) stop: SourceStop,
    pub(crate) runtime: Handle,
    pub(crate) position: u64,
    pub(crate) length: Option<u64>,
    pub(crate) seekable: bool,
    pub(crate) range_end: Option<u64>,
    pub(crate) body: Option<Body>,
    pub(crate) pending: Bytes,
    pub(crate) failure: Option<io::ErrorKind>,
}

pub(crate) enum Body {
    Local(std::fs::File),
    Remote(Box<AudioStream>),
}

impl Source {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn open(grant: PlaybackSource) -> Result<Self> {
        Self::open_with_stop(grant, SourceStop::new()).await
    }

    pub(crate) async fn open_with_stop(grant: PlaybackSource, stop: SourceStop) -> Result<Self> {
        let (body, length, seekable) = if grant.is_local() {
            let file = grant.open_file().await?;
            (Body::Local(file.file), Some(file.length), true)
        } else {
            let OpenedAudio::Content(mut audio) = grant.open(AudioRequest::default()).await? else {
                return Err(Error::Upstream(
                    "Audio did not return a complete representation",
                ));
            };
            let body = audio
                .body
                .take()
                .ok_or(Error::Upstream("Audio did not return content"))?;
            (
                Body::Remote(Box::new(body)),
                audio.total_length,
                audio.seekable,
            )
        };
        tracing::debug!(seekable, "Authorized native input opened");
        Ok(Self {
            grant,
            stop,
            runtime: Handle::current(),
            position: 0,
            length,
            seekable,
            range_end: None,
            body: Some(body),
            pending: Bytes::new(),
            failure: None,
        })
    }

    pub fn stop_handle(&self) -> SourceStop {
        self.stop.clone()
    }

    pub(crate) fn check(&self) -> io::Result<()> {
        if self.stop.requested() {
            return Err(interrupted());
        }
        if let Some(kind) = self.failure {
            return Err(io::Error::new(kind, "Audio input failed"));
        }
        self.runtime.block_on(async {
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => Err(interrupted()),
                result = self.grant.check() => result.map_err(|_| denied()),
            }
        })
    }
    fn read_next(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.check()?;
        if let Some(Body::Local(file)) = &mut self.body {
            let maximum = output.len().min(65536);
            let count = file.read(&mut output[..maximum])?;
            self.check()?;
            self.position = self
                .position
                .checked_add(count as u64)
                .ok_or_else(|| io::Error::other("Audio input position overflow"))?;
            return Ok(count);
        }
        while self.pending.is_empty() {
            let Some(Body::Remote(body)) = &mut self.body else {
                return Ok(0);
            };
            self.pending = self.runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = self.stop.cancelled() => Err(interrupted()),
                    _ = self.grant.revoked() => Err(denied()),
                    result = body.chunk() => result.map_err(|_| io::Error::other("Audio input failed")),
                }
            })?.unwrap_or_default();
            if self.pending.is_empty() {
                self.body = None;
                if self.range_end.and_then(|end| end.checked_add(1)) == Some(self.position)
                    && self.length.is_none_or(|length| self.position < length)
                {
                    self.open_at(self.position)?;
                    continue;
                }
                return Ok(0);
            }
        }
        self.check()?;
        let count = output.len().min(self.pending.len());
        output[..count].copy_from_slice(&self.pending.split_to(count));
        self.position = self
            .position
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("Audio input position overflow"))?;
        Ok(count)
    }
}

impl Read for Source {
    #[tracing::instrument(level = "trace", skip_all)]
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let result = self.read_next(output);
        if let Err(error) = &result
            && error.kind() != io::ErrorKind::Interrupted
        {
            self.failure = Some(error.kind());
            self.body = None;
            self.pending = Bytes::new();
        }
        result
    }
}

impl hito_audio::Input for Source {
    fn is_seekable(&self) -> bool {
        self.seekable
    }
    fn byte_len(&self) -> Option<u64> {
        self.length
    }
}

pub(crate) fn interrupted() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "Audio input stopped")
}
pub(crate) fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Audio authorization withdrawn",
    )
}
