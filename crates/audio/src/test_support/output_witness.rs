mod journal;

use crate::{AudioFormat, state::State};
use journal::Journal;
pub(crate) use journal::Writer;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};

static CURRENT: Mutex<Option<Arc<Journal>>> = Mutex::new(None);
static EXCLUSIVE: Mutex<()> = Mutex::new(());

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WitnessError {
    #[error("invalid output witness limits")]
    InvalidLimits,
    #[error("another output witness is active")]
    Busy,
    #[error("output writers have not joined")]
    ActiveWriters,
    #[error("output witness capacity exceeded")]
    Overflow,
    #[error("output witness contains an incomplete callback")]
    Incomplete,
}

#[derive(Debug)]
pub struct Callback {
    pub owner: u64,
    pub generation: u64,
    pub sequence: u64,
    pub offset: usize,
    pub frames: usize,
    pub sample_rate: u32,
    pub channels: u16,
    pub first: f32,
    pub last: f32,
    pub zeroes: usize,
    pub xruns: u64,
    pub epoch: u64,
    pub callback_ns: i64,
    pub playback_ns: i64,
}

#[derive(Debug)]
pub struct OutputReport {
    pub samples: Vec<f32>,
    pub callbacks: Vec<Callback>,
}

pub struct OutputWitness {
    journal: Arc<Journal>,
    _exclusive: MutexGuard<'static, ()>,
}

impl OutputWitness {
    pub fn begin(max_frames: usize, max_callbacks: usize) -> Result<Self, WitnessError> {
        if max_frames == 0
            || max_frames > 30 * 192_000
            || max_callbacks == 0
            || max_callbacks > 8192
        {
            return Err(WitnessError::InvalidLimits);
        }
        let exclusive = match EXCLUSIVE.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(WitnessError::Busy),
        };
        let journal = Journal::new(max_frames, max_callbacks);
        *lock(&CURRENT) = Some(journal.clone());
        Ok(Self {
            journal,
            _exclusive: exclusive,
        })
    }

    pub fn finish(&self) -> Result<OutputReport, WitnessError> {
        self.journal.finish()
    }
}

impl Drop for OutputWitness {
    fn drop(&mut self) {
        self.journal.seal();
        *lock(&CURRENT) = None;
    }
}

pub(crate) fn attach(state: &Arc<State>, format: AudioFormat) -> Option<Writer> {
    let journal = lock(&CURRENT).clone()?;
    journal.attach(state, format)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
