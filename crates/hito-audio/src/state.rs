use crate::{AudioFormat, Error};
use std::{
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    time::Duration,
};

pub struct Snapshot {
    pub playing: bool,
    pub ended: bool,
    pub position: Duration,
    pub speed: f32,
    pub format: AudioFormat,
    pub error: Option<Error>,
    pub underruns: u64,
    pub xruns: u64,
    pub output_failure: Option<&'static str>,
    pub notices: u64,
}

pub(crate) struct State {
    pub playing: AtomicBool,
    pub speed: AtomicU32,
    pub stopped: AtomicBool,
    pub seek_requested: AtomicBool,
    pub epoch: AtomicU64,
    pub position_epoch: AtomicU64,
    pub position: AtomicU64,
    pub base: AtomicU64,
    pub ended: AtomicU64,
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
            position_epoch: AtomicU64::new(0),
            position: AtomicU64::new(0),
            base: AtomicU64::new(0),
            ended: AtomicU64::new(0),
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
            _ => Some(Error::Worker),
        }
    }

    pub fn snapshot(&self, format: AudioFormat) -> Snapshot {
        let epoch = self.epoch.load(Ordering::Acquire);
        let position_epoch = self.position_epoch.load(Ordering::Acquire);
        let position = self.position.load(Ordering::Acquire);
        let position = if epoch == self.epoch.load(Ordering::Acquire) && position_epoch == epoch {
            position
        } else {
            self.base.load(Ordering::Acquire)
        };
        let ended = self.ended.load(Ordering::Acquire) == epoch;
        let error = self.error();
        Snapshot {
            playing: self.playing.load(Ordering::Acquire)
                && !self.stopped.load(Ordering::Acquire)
                && !ended
                && error.is_none(),
            ended,
            position: Duration::from_nanos(position),
            speed: f32::from_bits(self.speed.load(Ordering::Acquire)),
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
}
