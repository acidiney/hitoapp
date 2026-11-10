use super::{AudioFormat, Decoded};
use crate::{Error, Result};
use std::time::Duration;
use symphonia::core::audio::{Channels, Position};

pub trait PcmSource: Send {
    fn format(&self) -> AudioFormat;
    fn read(&mut self, interrupted: &mut dyn FnMut() -> bool) -> Result<Option<Decoded<'_>>>;
    fn seek(&mut self, position: Duration) -> Result<()>;
}

pub(super) struct Decoder {
    source: Box<dyn PcmSource>,
    format: AudioFormat,
    layout: Channels,
    at_end: bool,
}

impl Decoder {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) fn open(source: impl PcmSource + 'static) -> Result<Self> {
        let format = source.format();
        if !(8_000..=192_000).contains(&format.sample_rate) || !(1..=2).contains(&format.channels) {
            return Err(Error::Unsupported);
        }
        tracing::debug!(
            event = "pcm_decoder_ready",
            sample_rate = format.sample_rate,
            channels = format.channels,
            "PCM decoder ready"
        );
        let positions = if format.channels == 1 {
            Position::FRONT_CENTER
        } else {
            Position::FRONT_LEFT | Position::FRONT_RIGHT
        };
        Ok(Self {
            source: Box::new(source),
            format,
            layout: Channels::Positioned(positions),
            at_end: false,
        })
    }

    pub(super) fn format(&self) -> AudioFormat {
        self.format
    }
    pub(super) fn layout(&self) -> &Channels {
        &self.layout
    }
    #[tracing::instrument(level = "trace", skip_all)]
    pub(super) fn read(
        &mut self,
        interrupted: &mut dyn FnMut() -> bool,
    ) -> Result<Option<Decoded<'_>>> {
        if interrupted() {
            return Err(Error::Interrupted);
        }
        if self.at_end {
            return Ok(None);
        }
        let block = self.source.read(interrupted)?;
        if interrupted() {
            return Err(Error::Interrupted);
        }
        if let Some(block) = &block {
            let frames = block.samples.len() / usize::from(self.format.channels);
            let duration =
                Duration::from_secs_f64(frames as f64 / f64::from(self.format.sample_rate));
            if block.samples.is_empty()
                || block.samples.len() > 1_048_576
                || !block
                    .samples
                    .len()
                    .is_multiple_of(usize::from(self.format.channels))
                || block.samples.iter().any(|sample| !sample.is_finite())
                || block.position.checked_add(duration).is_none()
            {
                return Err(Error::Decode);
            }
        }
        self.at_end = block.is_none();
        Ok(block)
    }
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) fn seek(&mut self, position: Duration) -> Result<()> {
        self.source.seek(position)?;
        self.at_end = false;
        tracing::debug!(event = "pcm_decoder_seeked", "PCM decoder seeked");
        Ok(())
    }
}
