use crate::{AudioFile, Error, PlaybackSource, Result};
use bytes::Bytes;
use std::time::Instant;
use tokio::io::AsyncReadExt;

enum Body {
    Local(Box<LocalAudio>),
    Remote(Box<super::remote::RemoteAudio>),
    Lease(Box<crate::playback_access::leases::owner::LeaseBody>),
}

struct LocalAudio {
    file: tokio::io::Take<tokio::fs::File>,
    authority: PlaybackSource,
}

pub struct AudioStream {
    inner: Option<Body>,
    expected: Option<u64>,
    transferred: u64,
    started: Instant,
    outcome: &'static str,
    span: tracing::Span,
}
impl utoipa::PartialSchema for AudioStream {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::{KnownFormat, ObjectBuilder, SchemaFormat, Type};
        ObjectBuilder::new()
            .schema_type(Type::String)
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
            .into()
    }
}
impl utoipa::ToSchema for AudioStream {}
impl AudioStream {
    pub(super) async fn local(
        file: AudioFile,
        authority: PlaybackSource,
        start: u64,
        length: u64,
    ) -> Result<Self> {
        use tokio::io::AsyncSeekExt;
        let mut file = tokio::fs::File::from_std(file.file);
        authority
            .while_authorized(async {
                file.seek(std::io::SeekFrom::Start(start))
                    .await
                    .map_err(|_| Error::Internal("Audio file could not be read".into()))
            })
            .await?;
        Ok(Self::new(
            Body::Local(Box::new(LocalAudio {
                file: file.take(length),
                authority,
            })),
            Some(length),
        ))
    }
    pub(crate) fn remote(body: super::remote::RemoteAudio, length: Option<u64>) -> Self {
        Self::new(Body::Remote(Box::new(body)), length)
    }
    pub(crate) fn lease(
        body: crate::playback_access::leases::owner::LeaseBody,
        length: Option<u64>,
    ) -> Self {
        Self::new(Body::Lease(Box::new(body)), length)
    }
    fn new(inner: Body, expected: Option<u64>) -> Self {
        Self {
            inner: Some(inner),
            expected,
            transferred: 0,
            started: Instant::now(),
            outcome: "abandoned",
            span: tracing::Span::current(),
        }
    }
    #[tracing::instrument(level = "trace", skip_all)]
    pub async fn chunk(&mut self) -> Result<Option<Bytes>> {
        let Some(inner) = &mut self.inner else {
            return Ok(None);
        };
        let result = match inner {
            Body::Local(body) => body.chunk().await,
            Body::Remote(body) => body.chunk().await,
            Body::Lease(body) => body.chunk().await,
        };
        match result {
            Ok(Some(bytes)) => {
                self.transferred = self.transferred.saturating_add(bytes.len() as u64);
                if self
                    .expected
                    .is_some_and(|expected| self.transferred > expected)
                {
                    return Err(self.fail(Error::Upstream("Audio exceeded the declared length")));
                }
                Ok(Some(bytes))
            }
            Ok(None) => {
                if self
                    .expected
                    .is_some_and(|expected| expected != self.transferred)
                {
                    return Err(
                        self.fail(Error::Upstream("Audio ended before the declared length"))
                    );
                }
                self.finish("complete");
                Ok(None)
            }
            Err(error) => Err(self.fail(error)),
        }
    }
    fn fail(&mut self, error: Error) -> Error {
        self.finish("failed");
        let _entered = self.span.enter();
        tracing::warn!(
            event = "audio_stream_failed",
            error_kind = error.code(),
            bytes = self.transferred,
            elapsed_ms = self.started.elapsed().as_millis(),
            "Audio stream failed"
        );
        error
    }
    fn finish(&mut self, outcome: &'static str) {
        self.outcome = outcome;
        self.inner = None;
    }
}

impl LocalAudio {
    async fn chunk(&mut self) -> Result<Option<Bytes>> {
        let file = &mut self.file;
        self.authority
            .while_authorized(async {
                let mut bytes = vec![0; 65536];
                let read = file
                    .read(&mut bytes)
                    .await
                    .map_err(|_| Error::Internal("Audio file could not be read".into()))?;
                bytes.truncate(read);
                Ok((read > 0).then(|| Bytes::from(bytes)))
            })
            .await
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        let _entered = self.span.enter();
        tracing::debug!(
            outcome = self.outcome,
            bytes = self.transferred,
            elapsed_ms = self.started.elapsed().as_millis(),
            "Audio transfer closed"
        );
    }
}
