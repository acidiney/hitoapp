use crate::{AudioFormat, Error};
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering, fence};
#[cfg(not(loom))]
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering, fence};
use std::time::Duration;

pub struct Snapshot {
    pub playing: bool,
    pub buffer_ready: bool,
    pub ended: bool,
    pub position: Duration,
    pub speed: f32,
    pub track: u64,
    pub format: AudioFormat,
    pub error: Option<Error>,
    pub underruns: u64,
    pub xruns: u64,
    pub output_failure: Option<&'static str>,
    pub notices: u64,
}

pub(crate) struct State {
    pub playing: AtomicBool,
    speed: AtomicU32,
    pub stopped: AtomicBool,
    pub seek_requested: AtomicBool,
    pub epoch: AtomicU64,
    #[cfg(any(feature = "device", loom))]
    pub ready_epoch: AtomicU64,
    position_epoch: AtomicU64,
    position: AtomicU64,
    base: AtomicU64,
    track: AtomicU64,
    ended: AtomicU64,
    presented_sequence: AtomicU64,
    reset_sequence: AtomicU64,
    pub volume: AtomicU32,
    pub failure: AtomicU8,
    pub underruns: AtomicU64,
    pub xruns: AtomicU64,
    pub recovered_xruns: AtomicU64,
    pub callbacks: AtomicU64,
    pub output_failure: AtomicU8,
    pub notices: AtomicU64,
}

impl State {
    pub fn new() -> Self {
        Self {
            playing: AtomicBool::new(false),
            speed: AtomicU32::new(1_f32.to_bits()),
            stopped: AtomicBool::new(false),
            seek_requested: AtomicBool::new(false),
            epoch: AtomicU64::new(1),
            #[cfg(any(feature = "device", loom))]
            ready_epoch: AtomicU64::new(0),
            position_epoch: AtomicU64::new(0),
            position: AtomicU64::new(0),
            base: AtomicU64::new(0),
            track: AtomicU64::new(0),
            ended: AtomicU64::new(0),
            presented_sequence: AtomicU64::new(0),
            reset_sequence: AtomicU64::new(0),
            volume: AtomicU32::new(1_f32.to_bits()),
            failure: AtomicU8::new(0),
            underruns: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            recovered_xruns: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            output_failure: AtomicU8::new(0),
            notices: AtomicU64::new(0),
        }
    }

    pub fn fail(&self, error: Error) {
        let code = match error {
            Error::Unsupported => 1,
            Error::Decode => 2,
            Error::FormatChanged => 3,
            Error::Seek => 4,
            Error::OutputUnavailable => 5,
            Error::Output => 6,
            Error::Worker => 7,
            Error::InvalidControl => 8,
            Error::Interrupted => 9,
            Error::OutputFormat => 10,
        };
        self.failure.store(code, Ordering::Release);
        self.playing.store(false, Ordering::Release);
    }

    pub fn error(&self) -> Option<Error> {
        match self.failure.load(Ordering::Acquire) {
            0 => None,
            1 => Some(Error::Unsupported),
            2 => Some(Error::Decode),
            3 => Some(Error::FormatChanged),
            4 => Some(Error::Seek),
            5 => Some(Error::OutputUnavailable),
            6 => Some(Error::Output),
            8 => Some(Error::InvalidControl),
            9 => Some(Error::Interrupted),
            10 => Some(Error::OutputFormat),
            _ => Some(Error::Worker),
        }
    }

    pub fn snapshot(&self, format: AudioFormat) -> Snapshot {
        let (position, speed, ended, track, buffer_ready) = loop {
            if let Some(observation) = self.try_observation() {
                break observation;
            }
            std::hint::spin_loop();
        };
        let error = self.error();
        Snapshot {
            buffer_ready: cfg!(feature = "device") && buffer_ready,
            playing: self.playing.load(Ordering::Acquire)
                && !self.stopped.load(Ordering::Acquire)
                && !ended
                && error.is_none(),
            ended,
            position: Duration::from_nanos(position),
            speed,
            track,
            format,
            error,
            underruns: self.underruns.load(Ordering::Relaxed),
            xruns: self.xruns.load(Ordering::Relaxed),
            output_failure: match self.output_failure.load(Ordering::Acquire) {
                0 => None,
                1 => Some("backend"),
                2 => Some("device_unavailable"),
                3 => Some("stream_invalidated"),
                _ => Some("other"),
            },
            notices: self.notices.load(Ordering::Relaxed),
        }
    }

    fn try_observation(&self) -> Option<(u64, f32, bool, u64, bool)> {
        let presented = self.presented_sequence.load(Ordering::Acquire);
        let reset = self.reset_sequence.load(Ordering::Acquire);
        if !presented.is_multiple_of(2) || !reset.is_multiple_of(2) {
            return None;
        }
        let epoch = self.epoch.load(Ordering::Acquire);
        let position = if self.position_epoch.load(Ordering::Relaxed) == epoch {
            self.position.load(Ordering::Relaxed)
        } else {
            self.base.load(Ordering::Relaxed)
        };
        let speed = f32::from_bits(self.speed.load(Ordering::Relaxed));
        let ended = self.ended.load(Ordering::Relaxed) == epoch;
        let track = self.track.load(Ordering::Relaxed);
        #[cfg(any(feature = "device", loom))]
        let buffer_ready = self.ready_epoch.load(Ordering::Acquire) == epoch;
        #[cfg(not(any(feature = "device", loom)))]
        let buffer_ready = false;
        fence(Ordering::Acquire);
        (self.presented_sequence.load(Ordering::Relaxed) == presented
            && self.reset_sequence.load(Ordering::Relaxed) == reset)
            .then_some((position, speed, ended, track, buffer_ready))
    }

    pub fn present(&self, epoch: u64, position: u64, speed: Option<f32>, ended: bool, track: u64) {
        self.presented_sequence.fetch_add(1, Ordering::AcqRel);
        fence(Ordering::Release);
        self.position.store(position, Ordering::Relaxed);
        if let Some(speed) = speed {
            self.speed.store(speed.to_bits(), Ordering::Relaxed);
        }
        self.position_epoch.store(epoch, Ordering::Relaxed);
        self.track.store(track, Ordering::Relaxed);
        if ended {
            self.ended.store(epoch, Ordering::Relaxed);
        }
        self.presented_sequence.fetch_add(1, Ordering::Release);
    }

    pub fn reset(&self, position: u64) -> u64 {
        self.reset_sequence.fetch_add(1, Ordering::AcqRel);
        fence(Ordering::Release);
        self.base.store(position, Ordering::Relaxed);
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.reset_sequence.fetch_add(1, Ordering::Release);
        epoch
    }
}

#[cfg(all(test, loom))]
#[path = "state_model.rs"]
mod state_model;
