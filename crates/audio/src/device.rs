use crate::{
    AudioFormat, Error, Result, buffer_policy,
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

pub(crate) struct Output {
    stream: cpal::Stream,
    audio_id: u64,
    generation: u64,
}

fn playable(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    )
}

impl Output {
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

    pub fn start(
        format: AudioFormat,
        state: Arc<State>,
        audio_id: u64,
        generation: u64,
    ) -> Result<(rtrb::Producer<Frame>, Self)> {
        state.callbacks.store(0, Ordering::Release);
        state.rebuild_request.store(0, Ordering::Release);
        state.output_generation.store(generation, Ordering::Release);
        let (producer, consumer) = rtrb::RingBuffer::new((format.sample_rate / 10) as usize);
        let output = Self::open(format, consumer, state, audio_id, generation)?;
        Ok((producer, output))
    }

    fn open(
        format: AudioFormat,
        consumer: Consumer<Frame>,
        state: Arc<State>,
        audio_id: u64,
        generation: u64,
    ) -> Result<Self> {
        let opening = Instant::now();
        if format.channels > 2 {
            return Err(Error::Unsupported);
        }
        let host = cpal::default_host();
        let backend = host.id();
        let device = host
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
        #[cfg(target_os = "linux")]
        let linux_alsa = backend == cpal::HostId::Alsa;
        #[cfg(not(target_os = "linux"))]
        let linux_alsa = false;
        let request =
            buffer_policy::choose(linux_alsa, format.sample_rate, supported.buffer_size());
        config.buffer_size = request.buffer;
        let configured_quantum_frames = match request.buffer {
            cpal::BufferSize::Default => None,
            cpal::BufferSize::Fixed(frames) => Some(frames),
        };
        tracing::debug!(
            event = "audio_output_opening", audio_id, generation, backend = ?backend,
            requested_quantum_ms = request.milliseconds,
            requested_quantum_frames = request.frames,
            configured_quantum_frames, buffer_constraint = request.constraint,
            "Audio output quantum requested"
        );
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(
                &device,
                config,
                consumer,
                state.clone(),
                format,
                (audio_id, generation),
            ),
            SampleFormat::I16 => build::<i16>(
                &device,
                config,
                consumer,
                state.clone(),
                format,
                (audio_id, generation),
            ),
            SampleFormat::U16 => build::<u16>(
                &device,
                config,
                consumer,
                state.clone(),
                format,
                (audio_id, generation),
            ),
            _ => return Err(Error::OutputUnavailable),
        }?;
        if let Err(error) = stream.play() {
            tracing::warn!(
                event = "audio_output_failed",
                audio_id,
                generation,
                reason = "play_start",
                kind = failure_kind(error.kind()),
                terminal = true,
                "Audio output could not start"
            );
            return Err(Error::Output);
        }
        await_callbacks(&state, generation, Instant::now() + Duration::from_secs(2))?;
        if let Some(error) = state.error() {
            return Err(error);
        }
        tracing::info!(
            event = "audio_output_opened",
            audio_id, generation, backend = ?backend,
            requested_quantum_ms = request.milliseconds,
            requested_quantum_frames = request.frames,
            configured_quantum_frames, buffer_constraint = request.constraint,
            reported_callback_frames = stream.buffer_size().ok(),
            open_us = opening.elapsed().as_micros() as u64,
            sample_rate = format.sample_rate,
            channels = format.channels,
            "Audio output opened"
        );
        Ok(Self {
            stream,
            audio_id,
            generation,
        })
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    consumer: Consumer<Frame>,
    state: Arc<State>,
    format: AudioFormat,
    stream: (u64, u64),
) -> Result<cpal::Stream>
where
    f32: FromSample<T>,
{
    let mut render = Render::new(consumer, state.clone(), format).with_stream(stream);
    let origin = Origin::capture(stream);
    let born_xruns = state.xruns.load(Ordering::Acquire);
    #[cfg(feature = "test-support")]
    let mut witness = crate::test_support::output_witness::attach(&state, format);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info| {
                render.fill(data, info);
                #[cfg(feature = "test-support")]
                if let Some(witness) = &mut witness {
                    witness.record(data, info);
                }
            },
            move |error| report(&state, &origin, born_xruns, error.kind()),
            Some(Duration::from_secs(2)),
        )
        .map_err(|_| Error::OutputUnavailable)
}

fn await_callbacks(state: &State, generation: u64, deadline: Instant) -> Result<()> {
    while state.callbacks.load(Ordering::Acquire) < 2 {
        if let Some(error) = state.error() {
            return Err(error);
        }
        if state.rebuild_request.load(Ordering::Acquire) == generation
            || state.stopped.load(Ordering::Acquire)
        {
            return Err(Error::Output);
        }
        if Instant::now() >= deadline {
            return Err(Error::OutputUnavailable);
        }
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn failure_kind(kind: cpal::ErrorKind) -> &'static str {
    match kind {
        cpal::ErrorKind::BackendError => "backend",
        cpal::ErrorKind::DeviceNotAvailable => "device",
        cpal::ErrorKind::StreamInvalidated => "invalidated",
        _ => "other",
    }
}

struct Origin {
    stream: (u64, u64),
    dispatch: tracing::Dispatch,
}

impl Origin {
    fn capture(stream: (u64, u64)) -> Self {
        Self {
            stream,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        }
    }
}

fn report(state: &State, origin: &Origin, born_xruns: u64, kind: cpal::ErrorKind) {
    let (audio_id, generation) = origin.stream;
    let emit = |reason: &'static str, current_xruns: u64, action: &'static str, terminal: bool| {
        tracing::dispatcher::with_default(&origin.dispatch, || {
            tracing::warn!(
                event = "audio_output_failed",
                audio_id,
                generation,
                reason,
                born_xruns,
                current_xruns,
                action,
                terminal,
                "Audio output reported a failure"
            )
        });
    };
    if matches!(
        kind,
        cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied
    ) {
        state.notices.fetch_add(1, Ordering::Release);
        return;
    }
    if state.output_generation.load(Ordering::Acquire) != generation {
        if kind != cpal::ErrorKind::Xrun {
            emit(
                failure_kind(kind),
                state.xruns.load(Ordering::Acquire),
                "retired",
                false,
            );
        }
        return;
    }
    if kind == cpal::ErrorKind::Xrun {
        state.xruns.fetch_add(1, Ordering::Release);
        return;
    }
    let reason = failure_kind(kind);
    let code = match kind {
        cpal::ErrorKind::BackendError => 1,
        cpal::ErrorKind::DeviceNotAvailable => 2,
        cpal::ErrorKind::StreamInvalidated => 3,
        _ => 4,
    };
    state.output_failure.store(code, Ordering::Release);
    let current_xruns = state.xruns.load(Ordering::Acquire);
    if current_xruns != born_xruns {
        emit(reason, current_xruns, "recovering", false);
    } else if generation > 1 && kind == cpal::ErrorKind::BackendError {
        if state
            .rebuild_request
            .compare_exchange(0, generation, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            emit(reason, current_xruns, "rebuild", false);
        }
    } else {
        emit(reason, current_xruns, "stop", true);
        state.fail(Error::Output);
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.stream.pause();
        tracing::info!(
            event = "audio_output_closed",
            audio_id = self.audio_id,
            generation = self.generation,
            "Audio output closed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn owner(sink: &Sink) -> tracing::Dispatch {
        let writer = sink.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .json()
                .with_max_level(tracing::Level::INFO)
                .with_writer(move || writer.clone())
                .finish(),
        )
    }

    #[test]
    fn a_backend_failure_on_a_callback_thread_reaches_its_owners_subscriber() {
        let sink = Sink::default();
        let state = Arc::new(active(1, 0));
        tracing::dispatcher::with_default(&owner(&sink), || {
            let callback_state = state.clone();
            let origin = Origin::capture((7, 1));
            std::thread::spawn(move || {
                report(&callback_state, &origin, 0, cpal::ErrorKind::BackendError)
            })
            .join()
            .unwrap();
        });
        let logged = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let event = logged
            .lines()
            .find(|line| line.contains("\"event\":\"audio_output_failed\""))
            .expect("the owner's subscriber must receive the callback thread's failure event");
        for field in [
            "\"reason\":\"backend\"",
            "\"audio_id\":7",
            "\"generation\":1",
            "\"terminal\":true",
        ] {
            assert!(event.contains(field), "{field} missing from {event}");
        }
    }

    fn active(generation: u64, xruns: u64) -> State {
        let state = State::new();
        state.output_generation.store(generation, Ordering::Release);
        state.xruns.store(xruns, Ordering::Release);
        state
    }

    #[test]
    fn a_backend_failure_of_a_starting_replacement_requests_one_rebuild_instead_of_stopping() {
        let state = active(2, 1);
        let replacement = Origin::capture((1, 2));
        for _ in 0..6 {
            report(&state, &replacement, 1, cpal::ErrorKind::BackendError);
        }
        assert_eq!(state.error(), None);
        assert_eq!(state.rebuild_request.load(Ordering::Acquire), 2);
        assert_eq!(state.xruns.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_requested_rebuild_ends_a_replacements_startup_without_waiting_for_callbacks() {
        let state = active(2, 1);
        state.rebuild_request.store(2, Ordering::Release);
        let started = Instant::now();
        let result = await_callbacks(&state, 2, started + Duration::from_secs(2));
        let waited = started.elapsed();
        assert!(matches!(result, Err(Error::Output)), "{result:?}");
        assert!(waited < Duration::from_secs(1), "{waited:?}");
    }

    #[test]
    fn stopping_ends_a_replacements_startup_without_waiting_for_callbacks() {
        let state = active(2, 1);
        state.stopped.store(true, Ordering::Release);
        let started = Instant::now();
        let result = await_callbacks(&state, 2, started + Duration::from_secs(2));
        let waited = started.elapsed();
        assert!(result.is_err());
        assert!(waited < Duration::from_secs(1), "{waited:?}");
    }

    #[test]
    fn the_first_output_and_other_replacement_failures_stay_terminal() {
        let initial = active(1, 0);
        report(
            &initial,
            &Origin::capture((1, 1)),
            0,
            cpal::ErrorKind::BackendError,
        );
        assert_eq!(initial.error(), Some(Error::Output));
        assert_eq!(initial.rebuild_request.load(Ordering::Acquire), 0);
        for kind in [
            cpal::ErrorKind::DeviceNotAvailable,
            cpal::ErrorKind::StreamInvalidated,
        ] {
            let replacement = active(2, 1);
            report(&replacement, &Origin::capture((1, 2)), 1, kind);
            assert_eq!(replacement.error(), Some(Error::Output));
            assert_eq!(replacement.rebuild_request.load(Ordering::Acquire), 0);
        }
    }

    #[test]
    fn a_retired_output_cannot_stop_rebuild_or_count_against_its_replacement() {
        let state = active(3, 1);
        let retired = Origin::capture((1, 2));
        report(&state, &retired, 1, cpal::ErrorKind::BackendError);
        report(&state, &retired, 1, cpal::ErrorKind::Xrun);
        report(&state, &retired, 1, cpal::ErrorKind::DeviceNotAvailable);
        assert_eq!(state.error(), None);
        assert_eq!(state.rebuild_request.load(Ordering::Acquire), 0);
        assert_eq!(state.xruns.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_replacement_failure_is_terminal_before_recovery_is_acknowledged() {
        let state = active(1, 1);
        report(
            &state,
            &Origin::capture((1, 1)),
            1,
            cpal::ErrorKind::BackendError,
        );
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
        let state = active(1, 0);
        report(&state, &Origin::capture((1, 1)), 0, cpal::ErrorKind::Xrun);
        report(
            &state,
            &Origin::capture((1, 1)),
            0,
            cpal::ErrorKind::BackendError,
        );
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
            report(&state, &Origin::capture((1, 1)), 0, kind);
            assert_eq!(state.error(), None);
        }
    }
}
