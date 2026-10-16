use crate::{Decoder, Error, Result, device::Output, render::Frame, state::State};
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
        reply: SyncSender<Result<()>>,
    },
    Stop,
    Recover,
}

pub(crate) fn run(
    mut decoder: Decoder,
    commands: Receiver<Control>,
    state: Arc<State>,
    ready: SyncSender<Result<()>>,
) -> Result<()> {
    let format = decoder.format();
    let channels = usize::from(format.channels);
    let (mut producer, output) = Output::start(format, state.clone())?;
    let mut output = Some(output);
    let _ = ready.send(Ok(()));
    let mut epoch = 1;
    let mut xruns = 0;
    let mut notices = 0;
    let mut finished = false;
    let mut position = 0;
    let mut pending = None;
    let mut resets = 0;
    let mut last_reset = Duration::ZERO;
    'decode: loop {
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
                Control::Recover => (),
                Control::Seek {
                    position: target,
                    reply,
                } => {
                    let result = reposition(&mut decoder, &state, target);
                    if let Ok(next) = result {
                        epoch = next;
                    }
                    finished = false;
                    position = nanos(target);
                    state.seek_requested.store(false, Ordering::Release);
                    let _ = reply.send(result.map(|_| ()));
                    result?;
                }
            }
        }
        let reported = state.xruns.load(Ordering::Acquire);
        if reported != xruns {
            xruns = reported;
            let target = state.snapshot(format).position;
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
            position = nanos(target);
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
                position = nanos(
                    block.position
                        + Duration::from_secs_f64(
                            (index + 1) as f64 / f64::from(format.sample_rate),
                        ),
                );
                let mut audio = [0.; 2];
                audio[..channels].copy_from_slice(samples);
                if let Some(control) = send(
                    Frame::Audio {
                        samples: audio,
                        position,
                        epoch,
                    },
                    &mut producer,
                    &commands,
                    &state,
                    xruns,
                )? {
                    pending = Some(control);
                    continue 'decode;
                }
            }
        } else {
            if let Some(control) = send(
                Frame::End { position, epoch },
                &mut producer,
                &commands,
                &state,
                xruns,
            )? {
                pending = Some(control);
                continue;
            }
            finished = true;
        }
    }
}

fn reposition(decoder: &mut Decoder, state: &State, position: Duration) -> Result<u64> {
    decoder.seek(position)?;
    state.base.store(nanos(position), Ordering::Release);
    Ok(state.epoch.fetch_add(1, Ordering::AcqRel) + 1)
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
) -> Result<Option<Control>> {
    loop {
        if state.stopped.load(Ordering::Acquire) {
            return Ok(Some(Control::Stop));
        }
        if let Some(error) = state.error() {
            return Err(error);
        }
        if let Ok(control) = commands.try_recv() {
            return Ok(Some(control));
        }
        if state.xruns.load(Ordering::Acquire) != xruns {
            return Ok(Some(Control::Recover));
        }
        match producer.push(frame) {
            Ok(()) => return Ok(None),
            Err(rtrb::PushError::Full(value)) => frame = value,
        }
        if let Ok(control) = commands.recv_timeout(Duration::from_millis(2)) {
            return Ok(Some(control));
        }
    }
}
