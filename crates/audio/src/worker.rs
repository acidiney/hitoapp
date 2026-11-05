use crate::{
    AudioFormat, Converter, Decoder, Error, Result, device::Output, negotiate::Target,
    render::Frame, state::State, tempo::Tempo,
};
use rtrb::Producer;
use std::{
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{Receiver, RecvTimeoutError, SyncSender},
    },
    time::Duration,
};

pub(crate) enum Control {
    Seek {
        position: Duration,
        track: u64,
        reply: SyncSender<Result<()>>,
    },
    Next {
        decoder: Decoder,
        reply: SyncSender<Result<bool>>,
    },
    CancelNext {
        reply: SyncSender<bool>,
    },
    Speed {
        value: f32,
        reply: SyncSender<Result<()>>,
    },
    Stop,
    Wake,
    Recover,
}

pub(crate) fn run(
    decoder: Decoder,
    commands: Receiver<Control>,
    state: Arc<State>,
    ready: SyncSender<Result<AudioFormat>>,
    rates: Option<Vec<u32>>,
) -> Result<()> {
    let target = Output::negotiate(decoder.format().output_format(), rates.as_deref())?;
    let mut decoder = Tempo::new(Converter::with_output(
        decoder,
        target.sample_rate,
        target.channels,
    ));
    let format = decoder.format();
    let channels = usize::from(format.channels);
    let (mut producer, output) = Output::start(format, state.clone())?;
    let mut output = Some(output);
    let _ = ready.send(Ok(format));
    let mut epoch = 1;
    let mut xruns = 0;
    let mut notices = 0;
    let mut finished = false;
    let mut started = false;
    let mut pending = None;
    let mut deferred = Deferred::default();
    let mut resets = 0;
    let mut last_reset = Duration::ZERO;
    'decode: loop {
        if let Some(speed) = deferred.speed.take() {
            decoder.speed(speed);
        }
        for control in std::mem::take(&mut deferred.joins) {
            offer(control, &mut decoder, target, format, finished);
        }
        if state.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(error) = state.error() {
            return Err(error);
        }
        let reported_notices = state.notices.load(Ordering::Acquire);
        if reported_notices != notices {
            notices = reported_notices;
            tracing::debug!(
                event = "audio_output_notice",
                notices,
                "Audio backend reported a nonfatal notification"
            );
        }
        if let Some(control) = pending.take().or_else(|| commands.try_recv().ok()) {
            match control {
                Control::Stop => return Ok(()),
                Control::Recover | Control::Wake => (),
                Control::Seek {
                    position: wanted,
                    track: requested,
                    reply,
                } => {
                    if requested != decoder.track() || decoder.joining() {
                        state.seek_requested.store(false, Ordering::Release);
                        let _ = reply.send(Ok(()));
                        tracing::debug!(
                            event = "audio_seek_ignored_at_boundary",
                            "Seek ignored because playback is already moving to the next input"
                        );
                    } else {
                        let result = reposition(&mut decoder, &state, wanted);
                        if let Ok(next) = result {
                            epoch = next;
                        }
                        finished = false;
                        state.seek_requested.store(false, Ordering::Release);
                        let _ = reply.send(result.map(|_| ()));
                        result?;
                    }
                }
                control @ (Control::Next { .. } | Control::CancelNext { .. }) => {
                    offer(control, &mut decoder, target, format, finished);
                }
                Control::Speed { value, reply } => {
                    decoder.speed(value);
                    let _ = reply.send(Ok(()));
                }
            }
        }
        let reported = state.xruns.load(Ordering::Acquire);
        if reported != xruns {
            xruns = reported;
            let observed = state.snapshot(format);
            if observed.track != decoder.track() {
                return Err(Error::Output);
            }
            let target = observed.position;
            if target >= last_reset + Duration::from_millis(200) {
                resets = 0;
            }
            resets += 1;
            if resets > 3 {
                return Err(Error::Output);
            }
            last_reset = target;
            output.take();
            state.output_failure.store(0, Ordering::Release);
            epoch = reposition(&mut decoder, &state, target)?;
            finished = false;
            let (next, replacement) = Output::start(format, state.clone())?;
            producer = next;
            output = Some(replacement);
            state.recovered_xruns.store(xruns, Ordering::Release);
            tracing::warn!(
                event = "audio_output_recovered",
                xruns,
                "Audio output underrun recovered at the last presented position"
            );
        }
        if !started {
            if !state.playing.load(Ordering::Acquire) {
                pending = commands.recv_timeout(Duration::from_millis(100)).ok();
                continue;
            }
            started = true;
        }
        if finished {
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(control) => pending = Some(control),
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => (),
            }
            continue;
        }
        let block = match decoder.read_until(|| {
            state.stopped.load(Ordering::Acquire)
                || state.seek_requested.load(Ordering::Acquire)
                || state.xruns.load(Ordering::Acquire) != xruns
                || state.error().is_some()
        }) {
            Ok(block) => block,
            Err(Error::Interrupted) => {
                pending = commands.recv_timeout(Duration::from_millis(2)).ok();
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(block) = block {
            for (index, samples) in block.samples.chunks_exact(channels).enumerate() {
                let position = nanos(
                    (block.position + Duration::from_secs_f64((index + 1) as f64 * block.step))
                        .min(block.end),
                );
                let mut audio = [0.; 2];
                audio[..channels].copy_from_slice(samples);
                if let Some(control) = send(
                    Frame::Audio {
                        samples: audio,
                        position,
                        epoch,
                        speed: block.speed,
                        track: block.track,
                    },
                    &mut producer,
                    &commands,
                    &state,
                    xruns,
                    &mut deferred,
                )? {
                    pending = Some(control);
                    continue 'decode;
                }
            }
        } else {
            let position = nanos(decoder.end());
            if let Some(control) = send(
                Frame::End {
                    position,
                    epoch,
                    track: decoder.track(),
                },
                &mut producer,
                &commands,
                &state,
                xruns,
                &mut deferred,
            )? {
                pending = Some(control);
                continue;
            }
            finished = true;
        }
    }
}

fn join(next: Decoder, target: Target, current: AudioFormat, tempo: &mut Tempo) -> Result<bool> {
    let converter = Converter::with_output(next, target.sample_rate, target.channels);
    let format = converter.format();
    if format.sample_rate != current.sample_rate || format.channels != current.channels {
        return Err(Error::FormatChanged);
    }
    tempo.set_next(converter)
}

fn reposition(decoder: &mut Tempo, state: &State, position: Duration) -> Result<u64> {
    decoder.seek(position)?;
    Ok(state.reset(nanos(position)))
}

fn nanos(position: Duration) -> u64 {
    position.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn send(
    mut frame: Frame,
    producer: &mut Producer<Frame>,
    commands: &Receiver<Control>,
    state: &State,
    xruns: u64,
    deferred: &mut Deferred,
) -> Result<Option<Control>> {
    loop {
        if state.stopped.load(Ordering::Acquire) {
            return Ok(Some(Control::Stop));
        }
        if let Some(error) = state.error() {
            return Err(error);
        }
        if let Some(control) = commands
            .try_recv()
            .ok()
            .and_then(|control| defer(control, deferred))
        {
            return Ok(Some(control));
        }
        if state.xruns.load(Ordering::Acquire) != xruns {
            return Ok(Some(Control::Recover));
        }
        match producer.push(frame) {
            Ok(()) => return Ok(None),
            Err(rtrb::PushError::Full(value)) => frame = value,
        }
        if let Some(control) = commands
            .recv_timeout(Duration::from_millis(2))
            .ok()
            .and_then(|control| defer(control, deferred))
        {
            return Ok(Some(control));
        }
    }
}

#[derive(Default)]
struct Deferred {
    speed: Option<f32>,
    joins: Vec<Control>,
}

fn defer(control: Control, deferred: &mut Deferred) -> Option<Control> {
    match control {
        Control::Speed { value, reply } => {
            deferred.speed = Some(value);
            let _ = reply.send(Ok(()));
            None
        }
        Control::Wake => None,
        control @ (Control::Next { .. } | Control::CancelNext { .. }) => {
            deferred.joins.push(control);
            None
        }
        control => Some(control),
    }
}

fn offer(
    control: Control,
    decoder: &mut Tempo,
    target: Target,
    format: AudioFormat,
    finished: bool,
) {
    match control {
        Control::Next {
            decoder: next,
            reply,
        } => {
            let result = if finished {
                Err(Error::Interrupted)
            } else {
                join(next, target, format, decoder)
            };
            tracing::debug!(
                event = "audio_next_input_prepared",
                accepted = result.is_ok(),
                "Next audio input offered to the running stream"
            );
            let _ = reply.send(result);
        }
        Control::CancelNext { reply } => {
            let cancelled = decoder.clear_next();
            tracing::debug!(
                event = "audio_next_input_cancelled",
                cancelled,
                "Next audio input withdrawn"
            );
            let _ = reply.send(cancelled);
        }
        _ => (),
    }
}
