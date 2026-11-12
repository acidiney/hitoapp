use crate::{Source, SourceStop};
use hito_audio::{AudioFormat, Decoded, Decoder, Error, PcmSource, Result};
use hito_core::{PcmStream, PlaybackSource};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::runtime::Handle;

const CONTROL_POLL: Duration = Duration::from_millis(15);

pub(crate) fn decode(input: Source, interrupted: &mut dyn FnMut() -> bool) -> Result<Decoder> {
    let grant = input.grant.clone();
    let stop = input.stop.clone();
    let runtime = input.runtime.clone();
    if stop.requested() || interrupted() {
        return Err(Error::Interrupted);
    }
    match Decoder::open(input) {
        Ok(decoder) => Ok(decoder),
        Err(Error::Unsupported) if grant.is_local() => {
            tracing::debug!(
                event = "native_fallback_selected",
                source_id = grant.source_id(),
                "Trying the compatibility decoder"
            );
            Decoder::from_pcm(Pcm::open(grant, stop, runtime, interrupted)?)
        }
        Err(error) => Err(error),
    }
}

struct Pcm {
    grant: PlaybackSource,
    stop: SourceStop,
    runtime: Handle,
    format: AudioFormat,
    stream: Option<PcmStream>,
    samples: Vec<f32>,
    position: Duration,
    ended: bool,
}

impl Pcm {
    fn open(
        grant: PlaybackSource,
        stop: SourceStop,
        runtime: Handle,
        interrupted: &mut dyn FnMut() -> bool,
    ) -> Result<Self> {
        let started = Instant::now();
        let stream = runtime.block_on(open(&grant, &stop, Duration::ZERO, interrupted))?;
        let format = format(&stream);
        tracing::debug!(
            event = "native_fallback_ready",
            source_id = grant.source_id(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Compatibility decoder ready"
        );
        Ok(Self {
            grant,
            stop,
            runtime,
            format,
            stream: Some(stream),
            samples: Vec::new(),
            position: Duration::ZERO,
            ended: false,
        })
    }

    fn retire(&mut self) -> Result<()> {
        if let Some(stream) = self.stream.take() {
            self.runtime.block_on(stream.close()).map_err(failed)?;
        }
        self.samples.clear();
        Ok(())
    }
}

async fn open(
    grant: &PlaybackSource,
    stop: &SourceStop,
    position: Duration,
    interrupted: &mut dyn FnMut() -> bool,
) -> Result<PcmStream> {
    if stop.requested() || interrupted() {
        return Err(Error::Interrupted);
    }
    let mut opening = grant.begin_pcm(position).map_err(failed)?;
    let result = loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break Err(Error::Interrupted),
            result = opening.ready() => break result.map_err(failed),
            _ = tokio::time::sleep(CONTROL_POLL) => {
                if interrupted() { break Err(Error::Interrupted); }
            },
        }
    };
    match result {
        Ok(stream) => {
            if stop.requested() || interrupted() {
                stream.close().await.map_err(failed)?;
                return Err(Error::Interrupted);
            }
            Ok(stream)
        }
        Err(error) => {
            opening.close().await.map_err(failed)?;
            Err(error)
        }
    }
}

fn format(stream: &PcmStream) -> AudioFormat {
    let format = stream.format();
    AudioFormat {
        sample_rate: format.sample_rate,
        channels: format.channels,
        duration: format.duration,
    }
}

async fn interruptible<T>(
    operation: impl Future<Output = T>,
    stop: &SourceStop,
    interrupted: &mut dyn FnMut() -> bool,
) -> Result<T> {
    if stop.requested() || interrupted() {
        return Err(Error::Interrupted);
    }
    tokio::pin!(operation);
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(Error::Interrupted),
            result = &mut operation => return Ok(result),
            _ = tokio::time::sleep(CONTROL_POLL) => {
                if interrupted() { return Err(Error::Interrupted); }
            },
        }
    }
}

fn failed(error: hito_core::Error) -> Error {
    tracing::debug!(
        event = "native_fallback_failed",
        error_kind = error.code(),
        "Compatibility decoder could not continue"
    );
    match error {
        hito_core::Error::UnsupportedAudio | hito_core::Error::MediaToolsMissing => {
            Error::Unsupported
        }
        _ => Error::Decode,
    }
}

impl PcmSource for Pcm {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read(&mut self, interrupted: &mut dyn FnMut() -> bool) -> Result<Option<Decoded<'_>>> {
        if self.stop.requested() || interrupted() {
            return Err(Error::Interrupted);
        }
        if self.ended {
            return Ok(None);
        }
        let stream = self.stream.as_mut().ok_or(Error::Decode)?;
        let block = self
            .runtime
            .block_on(interruptible(stream.next_block(), &self.stop, interrupted))?
            .map_err(failed)?;
        let Some(block) = block else {
            self.ended = true;
            return Ok(None);
        };
        self.samples = block.samples;
        self.position = block.position;
        Ok(Some(Decoded {
            samples: &self.samples,
            position: self.position,
        }))
    }

    fn seek(&mut self, position: Duration) -> Result<()> {
        self.ended = false;
        self.retire()?;
        if self.stop.requested() {
            return Err(Error::Interrupted);
        }
        if self
            .format
            .duration
            .is_some_and(|duration| position >= duration)
        {
            self.position = position;
            self.ended = true;
            return Ok(());
        }
        let stream = self
            .runtime
            .block_on(open(&self.grant, &self.stop, position, &mut || false))?;
        if format(&stream) != self.format {
            self.runtime.block_on(stream.close()).map_err(failed)?;
            return Err(Error::FormatChanged);
        }
        self.stream = Some(stream);
        tracing::debug!(
            event = "native_fallback_seeked",
            source_id = self.grant.source_id(),
            "Compatibility decoder repositioned"
        );
        Ok(())
    }
}

impl Drop for Pcm {
    fn drop(&mut self) {
        let started = Instant::now();
        let result = self.retire();
        tracing::debug!(
            event = "native_fallback_closed",
            source_id = self.grant.source_id(),
            joined = result.is_ok(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Compatibility decoder reclaimed"
        );
    }
}

#[cfg(test)]
mod tests;
