mod writer;
use super::{
    PlayerPhase, Result,
    effects::{Key, Update},
    ownership::State,
};
use hito_core::{Actor, Collection};
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;
use writer::Writer;

const CHECKPOINT_CAPACITY: usize = 64;
const STOP_RESERVE: usize = 4;

pub(super) struct Report {
    pub scope: Uuid,
    pub failed: bool,
    pub rejected: bool,
}

#[derive(Clone)]
struct Checkpoint {
    id: Uuid,
    actor: Actor,
    key: Key,
    recording: Uuid,
    position: i64,
    speed: u16,
}
struct Sample {
    checkpoint: Checkpoint,
    playing: bool,
    seek_id: Option<Uuid>,
    dirty: bool,
}
pub(super) struct Progress {
    writer: Writer,
    current: Option<Sample>,
    sample_at: Instant,
}
impl Progress {
    pub fn new(collection: Collection) -> Self {
        Self {
            writer: Writer::new(collection),
            current: None,
            sample_at: Instant::now() + Duration::from_secs(5),
        }
    }
    pub fn barrier(&mut self) -> u64 {
        self.finish();
        self.writer.watermark()
    }
    pub fn settled(&self, watermark: u64) -> bool {
        self.writer.settled(watermark)
    }
    pub fn ready(&self) -> bool {
        self.writer.load() < CHECKPOINT_CAPACITY - STOP_RESERVE
    }
    pub fn can_play(&self, state: &State) -> bool {
        let key = Self::key(state);
        match &self.current {
            Some(sample) => Some(sample.checkpoint.key) == key,
            None => self.ready(),
        }
    }
    fn key(state: &State) -> Option<Key> {
        state.owner.as_ref()?.key()
    }
    pub fn sync(&mut self, state: &State) {
        if state
            .owner
            .as_ref()
            .is_some_and(|owned| !owned.authority_available)
            || self
                .current
                .as_ref()
                .is_some_and(|sample| Some(sample.checkpoint.key) != Self::key(state))
        {
            self.finish();
        }
    }
    pub fn observe(&mut self, state: &State, update: &Update) {
        if state
            .owner
            .as_ref()
            .is_none_or(|owned| !owned.authority_available)
        {
            return;
        }
        if Self::key(state) != Some(update.key) || update.phase != PlayerPhase::Ready {
            return;
        }
        let owned = state.owner.as_ref().unwrap();
        if !owned.listening(update) || owned.seek.map(|seek| seek.id) != update.seek_id {
            return;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|sample| sample.checkpoint.key != update.key)
        {
            self.finish();
            if self.current.is_some() {
                return;
            }
        }
        if self.current.is_none() && !update.playing && !update.ended && update.seek_id.is_none() {
            return;
        }
        let position = update.position_ms.min(i64::MAX as u64) as i64;
        let changed = self.current.as_ref().is_none_or(|sample| {
            sample.checkpoint.position != position
                || sample.seek_id != update.seek_id
                || sample.checkpoint.speed != update.speed
        });
        let terminal = update.ended
            || self
                .current
                .as_ref()
                .is_some_and(|sample| sample.playing && !update.playing)
            || self.current.as_ref().map(|sample| sample.seek_id) != Some(update.seek_id);
        let terminal = terminal
            || self
                .current
                .as_ref()
                .is_some_and(|sample| sample.checkpoint.speed != update.speed);
        let dirty = changed || self.current.as_ref().is_some_and(|sample| sample.dirty);
        self.current = Some(Sample {
            checkpoint: Checkpoint {
                id: Uuid::new_v4(),
                actor: owned.actor.clone(),
                key: update.key,
                recording: owned.queue.snapshot().current.unwrap().recording_id,
                position,
                speed: update.speed,
            },
            playing: update.playing,
            seek_id: update.seek_id,
            dirty,
        });
        if terminal {
            self.checkpoint();
        }
        if update.ended {
            self.finish();
        }
    }
    fn checkpoint(&mut self) {
        if let Some(sample) = &mut self.current
            && sample.dirty
            && self.writer.enqueue(sample.checkpoint.clone())
        {
            sample.dirty = false;
        }
    }
    fn finish(&mut self) {
        self.checkpoint();
        if self.current.as_ref().is_some_and(|sample| !sample.dirty) {
            self.current = None;
        }
    }
    pub async fn next(&mut self) -> Report {
        loop {
            tokio::select! {
                biased;
                report = self.writer.next() => return report,
                _ = tokio::time::sleep_until(self.sample_at) => {
                    self.sample_at = Instant::now() + Duration::from_secs(5);
                    if self.writer.load() == 0 { self.checkpoint(); }
                },
            }
        }
    }
    pub async fn close(&mut self) -> Result<()> {
        self.writer.shutdown();
        loop {
            self.finish();
            if self.current.is_none() && self.writer.load() == 0 {
                return Ok(());
            }
            Writer::result(&self.writer.next().await)?;
        }
    }
}
