use crate::{PlayerEntryRef, PlayerRepeat};
use rand::seq::SliceRandom;
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
    order: Option<Vec<Uuid>>,
}

impl Traversal {
    pub fn shuffled(&self) -> bool {
        self.order.is_some()
    }

    pub fn toggle_shuffle(&mut self, entries: &[PlayerEntryRef], current: Option<Uuid>) {
        self.order = if self.order.is_some() {
            None
        } else {
            let mut order = entries
                .iter()
                .map(|entry| entry.id)
                .filter(|id| Some(*id) != current)
                .collect::<Vec<_>>();
            order.shuffle(&mut rand::thread_rng());
            if let Some(current) = current {
                order.insert(0, current);
            }
            Some(order)
        };
        tracing::debug!(
            event = "player_shuffle_changed",
            enabled = self.shuffled(),
            "Shuffle mode changed"
        );
    }

    fn entries<'a>(&'a self, entries: &'a [PlayerEntryRef]) -> impl Iterator<Item = Uuid> + 'a {
        self.order.iter().flatten().copied().chain(
            entries
                .iter()
                .filter(|_| self.order.is_none())
                .map(|entry| entry.id),
        )
    }

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
        let next = self
            .entries(entries)
            .skip_while(|id| *id != current)
            .skip(1)
            .find(|entry| !self.failed.contains(entry));
        next.or_else(|| {
            (self.repeat == PlayerRepeat::All)
                .then(|| {
                    self.entries(entries)
                        .find(|entry| !self.failed.contains(entry))
                })
                .flatten()
        })
    }
}
