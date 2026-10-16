use crate::{PlayerEntryRef, PlayerRepeat};
use std::collections::HashSet;
use uuid::Uuid;

pub(super) enum Advance {
    Requested,
    Completed,
    Failed,
}

#[derive(Default)]
pub(super) struct Traversal {
    pub repeat: PlayerRepeat,
    failed: HashSet<Uuid>,
}

impl Traversal {
    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            PlayerRepeat::Off => PlayerRepeat::All,
            PlayerRepeat::All => PlayerRepeat::One,
            PlayerRepeat::One => PlayerRepeat::Off,
        };
        tracing::debug!(event = "player_repeat_changed", mode = ?self.repeat, "Repeat mode changed");
    }

    pub fn reset_failures(&mut self) {
        self.failed.clear();
    }

    pub fn remove(&mut self, entry: Uuid) {
        self.failed.remove(&entry);
    }

    pub fn next(
        &mut self,
        entries: &[PlayerEntryRef],
        current: Option<Uuid>,
        reason: Advance,
    ) -> Option<Uuid> {
        let current = current?;
        match reason {
            Advance::Completed if self.repeat == PlayerRepeat::One => return Some(current),
            Advance::Failed => {
                self.failed.insert(current);
            }
            Advance::Requested => self.reset_failures(),
            Advance::Completed => {}
        }
        let index = entries.iter().position(|entry| entry.id == current)?;
        let next = entries[index + 1..]
            .iter()
            .find(|entry| !self.failed.contains(&entry.id));
        next.or_else(|| {
            (self.repeat == PlayerRepeat::All)
                .then(|| {
                    entries[..=index]
                        .iter()
                        .find(|entry| !self.failed.contains(&entry.id))
                })
                .flatten()
        })
        .map(|entry| entry.id)
    }
}
