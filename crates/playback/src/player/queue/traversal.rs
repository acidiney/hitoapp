use crate::{PlayerEntryRef, PlayerRepeat};
use rand::seq::SliceRandom;
use std::collections::{HashMap, HashSet};
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
    positions: HashMap<Uuid, usize>,
}

impl Traversal {
    pub fn shuffled(&self) -> bool {
        self.order.is_some()
    }

    pub fn toggle_shuffle(&mut self, entries: &[PlayerEntryRef], current: Option<Uuid>) {
        self.order = if self.order.is_some() {
            None
        } else {
            Some(Self::shuffled_order(entries, current))
        };
        self.reindex();
        tracing::debug!(
            event = "player_shuffle_changed",
            enabled = self.shuffled(),
            "Shuffle mode changed"
        );
    }

    fn shuffled_order(entries: &[PlayerEntryRef], current: Option<Uuid>) -> Vec<Uuid> {
        let mut order = entries
            .iter()
            .map(|entry| entry.id)
            .filter(|id| Some(*id) != current)
            .collect::<Vec<_>>();
        order.shuffle(&mut rand::thread_rng());
        if let Some(current) = current {
            order.insert(0, current);
        }
        order
    }

    pub fn replace(&mut self, entries: &[PlayerEntryRef], current: Option<Uuid>) {
        self.reset_failures();
        if self.order.is_some() {
            self.order = Some(Self::shuffled_order(entries, current));
            self.reindex();
            tracing::debug!(
                event = "player_shuffle_rebuilt",
                count = entries.len(),
                "Shuffle order rebuilt for a new queue"
            );
        }
    }

    pub fn first(&self, entries: &[PlayerEntryRef]) -> Option<Uuid> {
        self.entries(entries, 0).next()
    }

    fn entries<'a>(
        &'a self,
        entries: &'a [PlayerEntryRef],
        offset: usize,
    ) -> impl Iterator<Item = Uuid> + 'a {
        let (shuffled, canonical) = match &self.order {
            Some(order) => (&order[offset..], &[][..]),
            None => (&[][..], &entries[offset..]),
        };
        shuffled
            .iter()
            .copied()
            .chain(canonical.iter().map(|entry| entry.id))
    }

    fn reindex(&mut self) {
        self.positions.clear();
        if let Some(order) = &self.order {
            self.positions
                .extend(order.iter().enumerate().map(|(index, id)| (*id, index)));
        }
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
        if let Some(order) = &mut self.order {
            order.retain(|id| *id != entry);
            self.reindex();
        }
    }

    pub fn append(&mut self, entries: &[PlayerEntryRef]) {
        if let Some(order) = &mut self.order {
            let mut added = entries.iter().map(|entry| entry.id).collect::<Vec<_>>();
            added.shuffle(&mut rand::thread_rng());
            self.positions.extend(
                added
                    .iter()
                    .enumerate()
                    .map(|(index, id)| (*id, order.len() + index)),
            );
            order.extend(added);
        }
    }

    pub fn next(
        &mut self,
        entries: &[PlayerEntryRef],
        current: Option<usize>,
        reason: Advance,
    ) -> Option<Uuid> {
        let index = current?;
        let current = entries.get(index)?.id;
        match reason {
            Advance::Completed if self.repeat == PlayerRepeat::One => return Some(current),
            Advance::Failed => {
                self.failed.insert(current);
            }
            Advance::Requested => self.reset_failures(),
            Advance::Completed => {}
        }
        let index = if self.order.is_some() {
            *self.positions.get(&current)?
        } else {
            index
        };
        let next = self
            .entries(entries, index + 1)
            .find(|entry| !self.failed.contains(entry));
        next.or_else(|| {
            (self.repeat == PlayerRepeat::All)
                .then(|| {
                    self.entries(entries, 0)
                        .take(index + 1)
                        .find(|entry| !self.failed.contains(entry))
                })
                .flatten()
        })
    }
}
