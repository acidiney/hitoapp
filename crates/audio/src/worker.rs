use crate::{
    AudioFormat, Converter, Decoder, Error, Result,
    buffering::Buffering,
    device::Output,
    negotiate::Target,
    render::Frame,
    state::State,
    tempo::{Handoff, Tempo},
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
    audio_id: u64,
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
    let mut output_generation = 1;
    let Some((mut producer, output)) =
        Output::start(format, state.clone(), audio_id, output_generation)?
    else {
        return if state.stopped.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(Error::Output)
        };
    };
    let mut output = Some(output);
    let mut buffering = Buffering::new(producer.buffer().capacity());
    let _ = ready.send(Ok(format));
    let mut epoch = 1;
    let mut xruns = 0;
    let mut notices = 0;
    let mut finished = false;
    let mut started = false;
    let mut pending = None;
    let mut deferred = Deferred {
        speed: None,
        handoff: decoder.handoff(),
        target,
        format,
    };
    let mut resets = 0;
    let mut last_reset = Duration::ZERO;
    'decode: loop {
        if let Some(speed) = deferred.speed.take() {
            decoder.speed(speed);
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
                    deferred.offer(control);
                }
                Control::Speed { value, reply } => {
                    decoder.speed(value);
                    let _ = reply.send(Ok(()));
                }
            }
        }
        let reported = state.xruns.load(Ordering::Acquire);
        let requested = state.rebuild_request.load(Ordering::Acquire) == output_generation;
        if reported != xruns || requested {
            xruns = reported;
            let observed = state.snapshot(format);
            if observed.track != decoder.track() {
                tracing::warn!(
                    event = "audio_output_failed",
                    audio_id,
                    generation = output_generation,
                    reason = "track_mismatch",
                    current_xruns = reported,
                    terminal = true,
                    "Audio output recovery could not continue"
                );
                return Err(Error::Output);
            }
            let target = observed.position;
            if target >= last_reset + Duration::from_millis(200) {
                resets = 0;
            }
            resets += 1;
            if resets > 3 {
                tracing::warn!(
                    event = "audio_output_failed",
                    audio_id,
                    generation = output_generation,
                    reason = "reset_budget",
                    resets,
                    current_xruns = reported,
                    since_last_reset_ms = target.saturating_sub(last_reset).as_millis() as u64,
                    terminal = true,
                    "Audio output recovery could not continue"
                );
                return Err(Error::Output);
            }
            last_reset = target;
            let previous_callbacks = state.callbacks.load(Ordering::Acquire);
            output.take();
            state.output_failure.store(0, Ordering::Release);
            epoch = reposition(&mut decoder, &state, target)?;
            finished = false;
            output_generation += 1;
            let Some((next, replacement)) = restart(
                &state,
                audio_id,
                &mut output_generation,
                &mut resets,
                reported,
                |generation| Output::start(format, state.clone(), audio_id, generation),
            )?
            else {
                return Ok(());
            };
            producer = next;
            buffering = Buffering::new(producer.buffer().capacity());
            output = Some(replacement);
            state.recovered_xruns.store(xruns, Ordering::Release);
            tracing::warn!(
                event = "audio_output_recovered",
                audio_id,
                xruns,
                target_position_ns = nanos(target),
                track = observed.track,
                recovery_attempt = resets,
                trigger = if requested {
                    "replacement_failure"
                } else {
                    "xrun"
                },
                previous_callbacks,
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
                || state.recovery_due(xruns, output_generation)
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
                    (xruns, output_generation),
                    &mut deferred,
                    &mut buffering,
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
                (xruns, output_generation),
                &mut deferred,
                &mut buffering,
            )? {
                pending = Some(control);
                continue;
            }
            finished = true;
        }
    }
}

fn restart<T>(
    state: &State,
    audio_id: u64,
    generation: &mut u64,
    resets: &mut u32,
    reported: u64,
    mut attempt: impl FnMut(u64) -> Result<Option<T>>,
) -> Result<Option<T>> {
    loop {
        if state.stopped.load(Ordering::Acquire) {
            return Ok(None);
        }
        let outcome = attempt(*generation);
        if state.stopped.load(Ordering::Acquire) {
            return Ok(None);
        }
        if let Some(error) = state.error() {
            return Err(error);
        }
        match outcome {
            Ok(Some(started)) => return Ok(Some(started)),
            Ok(None) if state.rebuild_request.load(Ordering::Acquire) == *generation => {
                *resets += 1;
                if *resets > 3 {
                    tracing::warn!(
                        event = "audio_output_failed",
                        audio_id,
                        generation = *generation,
                        reason = "reset_budget",
                        resets = *resets,
                        current_xruns = reported,
                        terminal = true,
                        "Audio output recovery could not continue"
                    );
                    return Err(Error::Output);
                }
                *generation += 1;
            }
            Ok(None) => return Err(Error::Output),
            Err(error) => return Err(error),
        }
    }
}

fn join(next: Decoder, target: Target, current: AudioFormat, handoff: &Handoff) -> Result<bool> {
    let converter = Converter::with_output(next, target.sample_rate, target.channels);
    let format = converter.format();
    if format.sample_rate != current.sample_rate || format.channels != current.channels {
        return Err(Error::FormatChanged);
    }
    handoff.set(converter)
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
    recovery: (u64, u64),
    deferred: &mut Deferred,
    buffering: &mut Buffering,
) -> Result<Option<Control>> {
    let (xruns, generation) = recovery;
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
        if state.recovery_due(xruns, generation) {
            return Ok(Some(Control::Recover));
        }
        match producer.push(frame) {
            Ok(()) => {
                buffering.queued(frame, state);
                return Ok(None);
            }
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

struct Deferred {
    speed: Option<f32>,
    handoff: Handoff,
    target: Target,
    format: AudioFormat,
}

impl Deferred {
    fn offer(&self, control: Control) {
        match control {
            Control::Next {
                decoder: next,
                reply,
            } => {
                let result = join(next, self.target, self.format, &self.handoff);
                tracing::debug!(
                    event = "audio_next_input_prepared",
                    accepted = result.is_ok(),
                    "Next audio input offered to the running stream"
                );
                let _ = reply.send(result);
            }
            Control::CancelNext { reply } => {
                let cancelled = self.handoff.clear();
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
}

fn defer(control: Control, deferred: &mut Deferred) -> Option<Control> {
    match control {
        Control::Speed { value, reply } => {
            deferred.speed = Some(value);
            deferred.handoff.announce(value);
            let _ = reply.send(Ok(()));
            None
        }
        Control::Wake => None,
        control @ (Control::Next { .. } | Control::CancelNext { .. }) => {
            deferred.offer(control);
            None
        }
        control => Some(control),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tempo::Tempo, test_support::wave};
    use std::sync::mpsc;

    #[test]
    fn a_full_output_ring_wakes_for_a_rebuild_request_of_its_generation() {
        let decoder = Tempo::new(Converter::with_output(
            Decoder::open(wave(48_000, 2, 4_800, |_, _| 0)).unwrap(),
            48_000,
            2,
        ));
        let format = decoder.format();
        let mut deferred = Deferred {
            speed: None,
            handoff: decoder.handoff(),
            target: Target {
                sample_rate: 48_000,
                channels: 2,
            },
            format,
        };
        let (mut producer, _consumer) = rtrb::RingBuffer::new(1);
        let frame = Frame::End {
            position: 0,
            epoch: 1,
            track: 0,
        };
        producer.push(frame).unwrap();
        let mut buffering = Buffering::new(1);
        let state = Arc::new(State::new());
        state.output_generation.store(2, Ordering::Release);
        state.rebuild_request.store(2, Ordering::Release);
        let (_commands_open, commands) = mpsc::sync_channel::<Control>(1);
        let (result, outcome) = mpsc::channel();
        let sender_state = state.clone();
        let sender = std::thread::spawn(move || {
            let woke = send(
                frame,
                &mut producer,
                &commands,
                &sender_state,
                (0, 2),
                &mut deferred,
                &mut buffering,
            );
            let _ = result.send(matches!(woke, Ok(Some(Control::Recover))));
        });
        let woke = outcome.recv_timeout(Duration::from_secs(3));
        state.stopped.store(true, Ordering::Release);
        sender.join().unwrap();
        assert_eq!(
            woke,
            Ok(true),
            "a full ring must wake for its rebuild request"
        );
    }

    struct Owned(u64);

    fn attempts(
        state: &State,
        resets: u32,
        outcome: impl Fn(&State, u64) -> Result<Option<()>>,
    ) -> (Result<Option<Owned>>, Vec<u64>, u32) {
        let mut generation = 2;
        let mut resets = resets;
        let mut seen = Vec::new();
        let result = restart(state, 1, &mut generation, &mut resets, 1, |generation| {
            seen.push(generation);
            state.rebuild_request.store(0, Ordering::Release);
            outcome(state, generation).map(|started| started.map(|()| Owned(generation)))
        });
        (result, seen, resets)
    }

    fn requested(state: &State, generation: u64) -> Result<Option<()>> {
        state.rebuild_request.store(generation, Ordering::Release);
        state.rebuild_request.store(generation, Ordering::Release);
        Ok(None)
    }

    #[test]
    fn a_persistent_replacement_failure_stops_after_three_attempts() {
        let state = State::new();
        let (result, seen, resets) = attempts(&state, 1, requested);
        assert!(matches!(result, Err(Error::Output)));
        assert_eq!(seen, [2, 3, 4]);
        assert_eq!(resets, 4);
    }

    #[test]
    fn a_later_replacement_that_starts_is_returned_to_its_owner() {
        let state = State::new();
        let (result, seen, _) = attempts(&state, 1, |state, generation| {
            if generation < 4 {
                requested(state, generation)
            } else {
                Ok(Some(()))
            }
        });
        assert!(matches!(result, Ok(Some(Owned(4)))));
        assert_eq!(seen, [2, 3, 4]);
    }

    #[test]
    fn a_replacement_failure_without_a_rebuild_request_is_not_retried() {
        let state = State::new();
        let (result, seen, resets) = attempts(&state, 1, |_, _| Err(Error::OutputUnavailable));
        assert!(matches!(result, Err(Error::OutputUnavailable)));
        assert_eq!(seen, [2]);
        assert_eq!(resets, 1);
    }

    #[test]
    fn stopping_during_a_failed_replacement_start_launches_no_further_output() {
        let state = State::new();
        let (result, seen, _) = attempts(&state, 1, |state, generation| {
            state.stopped.store(true, Ordering::Release);
            requested(state, generation)
        });
        assert!(matches!(result, Ok(None)), "{:?}", result.err());
        assert_eq!(seen, [2]);
    }

    #[test]
    fn a_stopped_player_starts_no_replacement_output() {
        let state = State::new();
        state.stopped.store(true, Ordering::Release);
        let (result, seen, _) = attempts(&state, 1, |_, _| Ok(Some(())));
        assert!(matches!(result, Ok(None)));
        assert!(seen.is_empty());
    }

    #[test]
    fn a_terminal_failure_wins_over_a_pending_rebuild_of_the_same_generation() {
        let state = State::new();
        let (result, seen, _) = attempts(&state, 1, |state, generation| {
            let _ = requested(state, generation);
            state.fail(Error::Output);
            Ok(None)
        });
        assert!(matches!(result, Err(Error::Output)));
        assert_eq!(seen, [2]);
    }

    #[test]
    fn a_fatal_start_error_is_not_retried_while_a_rebuild_is_pending() {
        let state = State::new();
        let (result, seen, _) = attempts(&state, 1, |state, generation| {
            let _ = requested(state, generation);
            Err(Error::Output)
        });
        assert!(matches!(result, Err(Error::Output)));
        assert_eq!(seen, [2]);
    }
}
