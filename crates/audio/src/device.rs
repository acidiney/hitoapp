use crate::{
    AudioFormat, Error, Result,
    negotiate::{self, OutputConfig, Target},
    render::{Frame, Render},
    state::State,
};
use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rtrb::Consumer;
use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

pub(crate) struct Output(cpal::Stream);

fn playable(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    )
}

impl Output {
    /// Chooses the representation to open the default device with for a source
    /// already reduced to `wanted`. `rates` replaces the device's offered rates
    /// with a fixed list, for tests that need a device of a known capability.
    pub fn negotiate(wanted: AudioFormat, rates: Option<&[u32]>) -> Result<Target> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(Error::OutputUnavailable)?;
        let mut offered: Vec<OutputConfig> = device
            .supported_output_configs()
            .map_err(|_| Error::OutputUnavailable)?
            .filter(|config| playable(config.sample_format()))
            .map(|config| OutputConfig {
                channels: config.channels(),
                min_rate: config.min_sample_rate(),
                max_rate: config.max_sample_rate(),
            })
            .collect();
        if offered.is_empty() {
            return Err(Error::OutputUnavailable);
        }
        let mut default_rate = device
            .default_output_config()
            .ok()
            .map(|config| config.sample_rate());
        if let Some(rates) = rates {
            offered = offered
                .iter()
                .flat_map(|config| {
                    rates
                        .iter()
                        .filter(|rate| (config.min_rate..=config.max_rate).contains(rate))
                        .map(|rate| OutputConfig {
                            channels: config.channels,
                            min_rate: *rate,
                            max_rate: *rate,
                        })
                })
                .collect();
            default_rate = rates.first().copied();
        }
        let target = negotiate::choose(wanted.sample_rate, wanted.channels, &offered, default_rate);
        tracing::debug!(
            event = "audio_output_negotiated",
            source_rate = wanted.sample_rate,
            source_channels = wanted.channels,
            offered_configs = offered.len(),
            default_rate,
            chosen_rate = target.map(|target| target.sample_rate),
            chosen_channels = target.map(|target| target.channels),
            "Audio output representation negotiated"
        );
        target.ok_or(Error::OutputFormat)
    }

    pub fn start(format: AudioFormat, state: Arc<State>) -> Result<(rtrb::Producer<Frame>, Self)> {
        state.callbacks.store(0, Ordering::Release);
        let (producer, consumer) = rtrb::RingBuffer::new((format.sample_rate / 10) as usize);
        let output = Self::open(format, consumer, state)?;
        Ok((producer, output))
    }

    pub fn open(format: AudioFormat, consumer: Consumer<Frame>, state: Arc<State>) -> Result<Self> {
        if format.channels > 2 {
            return Err(Error::Unsupported);
        }
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(Error::OutputUnavailable)?;
        let supported = device
            .supported_output_configs()
            .map_err(|_| Error::OutputUnavailable)?
            .filter(|config| {
                config.channels() == format.channels
                    && config.min_sample_rate() <= format.sample_rate
                    && config.max_sample_rate() >= format.sample_rate
            })
            .filter(|config| playable(config.sample_format()))
            .max_by_key(|config| match config.sample_format() {
                SampleFormat::F32 => 3,
                SampleFormat::I16 => 2,
                _ => 1,
            })
            .ok_or(Error::OutputUnavailable)?
            .with_sample_rate(format.sample_rate);
        let mut config = supported.config();
        if let cpal::SupportedBufferSize::Range { min, max } = *supported.buffer_size() {
            config.buffer_size = cpal::BufferSize::Fixed((format.sample_rate / 50).clamp(min, max));
        }
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config, consumer, state.clone(), format),
            SampleFormat::I16 => build::<i16>(&device, config, consumer, state.clone(), format),
            SampleFormat::U16 => build::<u16>(&device, config, consumer, state.clone(), format),
            _ => return Err(Error::OutputUnavailable),
        }?;
        stream.play().map_err(|_| Error::Output)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.callbacks.load(Ordering::Acquire) < 2 {
            if let Some(error) = state.error() {
                return Err(error);
            }
            if Instant::now() >= deadline {
                return Err(Error::OutputUnavailable);
            }
            thread::sleep(Duration::from_millis(1));
        }
        if let Some(error) = state.error() {
            return Err(error);
        }
        tracing::info!(
            event = "audio_output_opened",
            sample_rate = format.sample_rate,
            channels = format.channels,
            "Audio output opened"
        );
        Ok(Self(stream))
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    consumer: Consumer<Frame>,
    state: Arc<State>,
    format: AudioFormat,
) -> Result<cpal::Stream> {
    let mut render = Render::new(consumer, state.clone(), format);
    let born_xruns = state.xruns.load(Ordering::Acquire);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info| render.fill(data, info),
            move |error| report(&state, born_xruns, error.kind()),
            Some(Duration::from_secs(2)),
        )
        .map_err(|_| Error::OutputUnavailable)
}

fn report(state: &State, born_xruns: u64, kind: cpal::ErrorKind) {
    if matches!(
        kind,
        cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied
    ) {
        state.notices.fetch_add(1, Ordering::Release);
    } else if kind == cpal::ErrorKind::Xrun {
        state.xruns.fetch_add(1, Ordering::Release);
    } else {
        let code = match kind {
            cpal::ErrorKind::BackendError => 1,
            cpal::ErrorKind::DeviceNotAvailable => 2,
            cpal::ErrorKind::StreamInvalidated => 3,
            _ => 4,
        };
        state.output_failure.store(code, Ordering::Release);
        if state.xruns.load(Ordering::Acquire) == born_xruns {
            state.fail(Error::Output);
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.0.pause();
        tracing::info!(event = "audio_output_closed", "Audio output closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replacement_failure_is_terminal_before_recovery_is_acknowledged() {
        let state = State::new();
        state.xruns.store(1, Ordering::Release);
        report(&state, 1, cpal::ErrorKind::BackendError);
        assert_eq!(state.error(), Some(Error::Output));
        assert_eq!(
            state
                .snapshot(AudioFormat {
                    sample_rate: 48_000,
                    channels: 2,
                    duration: None
                })
                .output_failure,
            Some("backend")
        );
    }

    #[test]
    fn follow_on_failure_of_the_old_stream_does_not_stop_its_replacement() {
        let state = State::new();
        report(&state, 0, cpal::ErrorKind::Xrun);
        report(&state, 0, cpal::ErrorKind::BackendError);
        assert_eq!(state.xruns.load(Ordering::Acquire), 1);
        assert_eq!(state.error(), None);
    }

    #[test]
    fn backend_notifications_do_not_stop_a_working_stream() {
        for kind in [
            cpal::ErrorKind::DeviceChanged,
            cpal::ErrorKind::RealtimeDenied,
        ] {
            let state = State::new();
            report(&state, 0, kind);
            assert_eq!(state.error(), None);
        }
    }
}
