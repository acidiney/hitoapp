use crate::{AudioFile, Error, Result};
use bytes::Bytes;
use std::time::Instant;
use tokio::io::AsyncReadExt;

enum Body {
    Local(tokio::io::Take<tokio::fs::File>),
    Remote(Box<crate::plugins::RemoteAudio>),
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
    pub(super) async fn local(file: AudioFile, start: u64, length: u64) -> Result<Self> {
        use tokio::io::AsyncSeekExt;
        let mut file = tokio::fs::File::from_std(file.file);
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|_| Error::Internal("Audio file could not be read".into()))?;
        Ok(Self::new(Body::Local(file.take(length)), Some(length)))
    }
    pub(crate) fn remote(body: crate::plugins::RemoteAudio, length: Option<u64>) -> Self {
        Self::new(Body::Remote(Box::new(body)), length)
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
    #[tracing::instrument(level = "trace", skip_all, err(Debug))]
    pub async fn chunk(&mut self) -> Result<Option<Bytes>> {
        let Some(inner) = &mut self.inner else {
            return Ok(None);
        };
        let result = match inner {
            Body::Local(file) => {
                let mut bytes = vec![0; 65536];
                file.read(&mut bytes)
                    .await
                    .map(|read| {
                        bytes.truncate(read);
                        (read > 0).then(|| Bytes::from(bytes))
                    })
                    .map_err(|_| Error::Internal("Audio file could not be read".into()))
            }
            Body::Remote(body) => body.chunk().await,
        };
        match result {
            Ok(Some(bytes)) => {
                self.transferred = self.transferred.saturating_add(bytes.len() as u64);
                if self
                    .expected
                    .is_some_and(|expected| self.transferred > expected)
                {
                    self.finish("failed");
                    return Err(Error::Upstream("Audio exceeded the declared length"));
                }
                Ok(Some(bytes))
            }
            Ok(None) => {
                if self
                    .expected
                    .is_some_and(|expected| expected != self.transferred)
                {
                    self.finish("failed");
                    return Err(Error::Upstream("Audio ended before the declared length"));
                }
                self.finish("complete");
                Ok(None)
            }
            Err(error) => {
                self.finish("failed");
                Err(error)
            }
        }
    }
    fn finish(&mut self, outcome: &'static str) {
        self.outcome = outcome;
        self.inner = None;
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
