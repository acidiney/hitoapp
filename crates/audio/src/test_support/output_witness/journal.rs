use super::{Callback, OutputReport, WitnessError};
use crate::{AudioFormat, state::State};
use cpal::{FromSample, OutputCallbackInfo, Sample, StreamInstant};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

#[derive(Default)]
struct Header {
    committed: AtomicU64,
    owner: AtomicU64,
    generation: AtomicU64,
    sequence: AtomicU64,
    offset: AtomicU64,
    frames: AtomicU64,
    sample_rate: AtomicU64,
    channels: AtomicU64,
    first: AtomicU64,
    last: AtomicU64,
    zeroes: AtomicU64,
    xruns: AtomicU64,
    epoch: AtomicU64,
    callback_ns: AtomicU64,
    playback_ns: AtomicU64,
}

#[derive(Default)]
struct Registration {
    sealed: bool,
    owners: Vec<(Weak<State>, u64)>,
}

pub(super) struct Journal {
    samples: Box<[AtomicU32]>,
    headers: Box<[Header]>,
    frames: AtomicUsize,
    callbacks: AtomicUsize,
    active: AtomicUsize,
    overflow: AtomicBool,
    invalid: AtomicBool,
    registration: Mutex<Registration>,
}

pub(crate) struct Writer {
    journal: Arc<Journal>,
    state: Arc<State>,
    format: AudioFormat,
    owner: u64,
    generation: u64,
    sequence: u64,
    origin: Option<StreamInstant>,
}

impl Journal {
    pub fn new(frames: usize, callbacks: usize) -> Arc<Self> {
        Arc::new(Self {
            samples: (0..frames).map(|_| AtomicU32::new(0)).collect(),
            headers: (0..callbacks).map(|_| Header::default()).collect(),
            frames: AtomicUsize::new(0),
            callbacks: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
            invalid: AtomicBool::new(false),
            registration: Mutex::default(),
        })
    }

    pub fn attach(self: &Arc<Self>, state: &Arc<State>, format: AudioFormat) -> Option<Writer> {
        let mut registration = super::lock(&self.registration);
        if registration.sealed {
            return None;
        }
        let weak = Arc::downgrade(state);
        let owner = match registration
            .owners
            .iter()
            .position(|(seen, _)| seen.ptr_eq(&weak))
        {
            Some(index) => index,
            None => {
                if registration.owners.len() == self.headers.len() {
                    self.overflow.store(true, Ordering::Release);
                    return None;
                }
                registration.owners.push((weak, 0));
                registration.owners.len() - 1
            }
        };
        registration.owners[owner].1 += 1;
        self.active.fetch_add(1, Ordering::AcqRel);
        Some(Writer {
            journal: self.clone(),
            state: state.clone(),
            format,
            owner: owner as u64 + 1,
            generation: registration.owners[owner].1,
            sequence: 0,
            origin: None,
        })
    }

    pub fn seal(&self) {
        super::lock(&self.registration).sealed = true;
    }

    pub fn finish(&self) -> Result<OutputReport, WitnessError> {
        {
            let mut registration = super::lock(&self.registration);
            if self.active.load(Ordering::Acquire) != 0 {
                return Err(WitnessError::ActiveWriters);
            }
            registration.sealed = true;
        }
        if self.overflow.load(Ordering::Acquire) {
            return Err(WitnessError::Overflow);
        }
        if self.invalid.load(Ordering::Acquire) {
            return Err(WitnessError::Incomplete);
        }
        let count = self.callbacks.load(Ordering::Acquire);
        if count == 0 || count > self.headers.len() {
            return Err(WitnessError::Incomplete);
        }
        let used = self.frames.load(Ordering::Acquire);
        let mut report = OutputReport {
            samples: Vec::with_capacity(used),
            callbacks: Vec::with_capacity(count),
        };
        for (index, header) in self.headers[..count].iter().enumerate() {
            if header.committed.load(Ordering::Acquire) != index as u64 + 1 {
                return Err(WitnessError::Incomplete);
            }
            let start = header.offset.load(Ordering::Relaxed) as usize;
            let frames = header.frames.load(Ordering::Relaxed) as usize;
            let end = start
                .checked_add(frames)
                .filter(|end| *end <= used && *end <= self.samples.len())
                .ok_or(WitnessError::Incomplete)?;
            let offset = report.samples.len();
            report.samples.extend(
                self.samples[start..end]
                    .iter()
                    .map(|value| f32::from_bits(value.load(Ordering::Relaxed))),
            );
            report.callbacks.push(Callback {
                owner: header.owner.load(Ordering::Relaxed),
                generation: header.generation.load(Ordering::Relaxed),
                sequence: header.sequence.load(Ordering::Relaxed),
                offset,
                frames,
                sample_rate: header.sample_rate.load(Ordering::Relaxed) as u32,
                channels: header.channels.load(Ordering::Relaxed) as u16,
                first: f32::from_bits(header.first.load(Ordering::Relaxed) as u32),
                last: f32::from_bits(header.last.load(Ordering::Relaxed) as u32),
                zeroes: header.zeroes.load(Ordering::Relaxed) as usize,
                xruns: header.xruns.load(Ordering::Relaxed),
                epoch: header.epoch.load(Ordering::Relaxed),
                callback_ns: header.callback_ns.load(Ordering::Relaxed) as i64,
                playback_ns: header.playback_ns.load(Ordering::Relaxed) as i64,
            });
        }
        if report.samples.len() != used {
            return Err(WitnessError::Incomplete);
        }
        Ok(report)
    }
}

impl Writer {
    pub fn record<T: Sample>(&mut self, data: &[T], info: &OutputCallbackInfo)
    where
        f32: FromSample<T>,
    {
        let journal = &self.journal;
        if journal.overflow.load(Ordering::Relaxed) || journal.invalid.load(Ordering::Relaxed) {
            return;
        }
        let channels = usize::from(self.format.channels);
        if channels == 0 || !data.len().is_multiple_of(channels) {
            journal.invalid.store(true, Ordering::Release);
            return;
        }
        let count = data.len() / channels;
        let index = journal.callbacks.fetch_add(1, Ordering::Relaxed);
        let offset = journal.frames.fetch_add(count, Ordering::Relaxed);
        if index >= journal.headers.len()
            || offset
                .checked_add(count)
                .is_none_or(|end| end > journal.samples.len())
        {
            journal.overflow.store(true, Ordering::Release);
            return;
        }
        let timestamp = info.timestamp();
        let origin = *self.origin.get_or_insert(timestamp.callback);
        self.sequence += 1;
        let mut first = 0_f32;
        let mut last = 0_f32;
        let mut zeroes = 0;
        for (frame, input) in data.chunks_exact(channels).enumerate() {
            let value: f32 = input[0].to_sample();
            if frame == 0 {
                first = value;
            }
            last = value;
            zeroes += usize::from(value == 0.);
            journal.samples[offset + frame].store(value.to_bits(), Ordering::Relaxed);
        }
        let header = &journal.headers[index];
        header.owner.store(self.owner, Ordering::Relaxed);
        header.generation.store(self.generation, Ordering::Relaxed);
        header.sequence.store(self.sequence, Ordering::Relaxed);
        header.offset.store(offset as u64, Ordering::Relaxed);
        header.frames.store(count as u64, Ordering::Relaxed);
        header
            .sample_rate
            .store(u64::from(self.format.sample_rate), Ordering::Relaxed);
        header
            .channels
            .store(u64::from(self.format.channels), Ordering::Relaxed);
        header
            .first
            .store(u64::from(first.to_bits()), Ordering::Relaxed);
        header
            .last
            .store(u64::from(last.to_bits()), Ordering::Relaxed);
        header.zeroes.store(zeroes as u64, Ordering::Relaxed);
        header
            .xruns
            .store(self.state.xruns.load(Ordering::Acquire), Ordering::Relaxed);
        header
            .epoch
            .store(self.state.epoch.load(Ordering::Acquire), Ordering::Relaxed);
        header
            .callback_ns
            .store(delta(timestamp.callback, origin) as u64, Ordering::Relaxed);
        header
            .playback_ns
            .store(delta(timestamp.playback, origin) as u64, Ordering::Relaxed);
        header.committed.store(index as u64 + 1, Ordering::Release);
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.journal.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn delta(at: StreamInstant, origin: StreamInstant) -> i64 {
    (at.as_nanos() as i128 - origin.as_nanos() as i128)
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
