use crate::{PlayerEntryRef, PlayerRepeat};
use rand::Rng;
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

    pub fn append(&mut self, entries: &[PlayerEntryRef], current: Option<Uuid>) {
        let Some(order) = &mut self.order else {
            return;
        };
        let from = current
            .and_then(|id| self.positions.get(&id))
            .map_or(order.len(), |index| index + 2)
            .min(order.len());
        let mut rng = rand::thread_rng();
        let mut added = entries.iter().map(|entry| entry.id).collect::<Vec<_>>();
        added.shuffle(&mut rng);
        let tail = order.split_off(from);
        let (mut left, mut right) = (tail.len(), added.len());
        let (mut tail, mut added) = (tail.into_iter(), added.into_iter());
        order.reserve(left + right);
        while left + right > 0 {
            let next = if rng.gen_range(0..left + right) < left {
                left -= 1;
                tail.next()
            } else {
                right -= 1;
                added.next()
            };
            order.extend(next);
        }
        for (index, id) in order.iter().enumerate().skip(from) {
            self.positions.insert(*id, index);
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
        self.following(entries, index)
    }

    #[cfg(feature = "device")]
    pub fn upcoming(&self, entries: &[PlayerEntryRef], current: Option<usize>) -> Option<Uuid> {
        let index = current?;
        let current = entries.get(index)?.id;
        if self.repeat == PlayerRepeat::One {
            return Some(current);
        }
        self.following(entries, index)
    }

    fn following(&self, entries: &[PlayerEntryRef], index: usize) -> Option<Uuid> {
        let current = entries.get(index)?.id;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(count: usize) -> Vec<PlayerEntryRef> {
        (0..count)
            .map(|_| PlayerEntryRef {
                id: Uuid::new_v4(),
                recording_id: Uuid::new_v4(),
                chapter: None,
            })
            .collect()
    }

    #[test]
    fn appended_entries_mix_into_the_unplayed_part_of_a_shuffled_order() {
        let first = entries(200);
        let added = entries(200);
        let all = first.iter().chain(&added).cloned().collect::<Vec<_>>();
        let mut traversal = Traversal::default();
        traversal.toggle_shuffle(&first, Some(first[0].id));
        traversal.append(&added, Some(first[0].id));

        let mut visited = vec![first[0].id];
        let mut current = 0;
        while let Some(next) = traversal.next(&all, Some(current), Advance::Completed) {
            current = all.iter().position(|entry| entry.id == next).unwrap();
            visited.push(next);
        }

        assert_eq!(visited.len(), 400);
        assert_eq!(visited[0], first[0].id);
        assert_eq!(visited.iter().collect::<HashSet<_>>().len(), 400);
        let later = added.iter().map(|entry| entry.id).collect::<HashSet<_>>();
        assert!(
            visited[1..200].iter().any(|id| later.contains(id)),
            "Entries appended later were all held back until the first batch finished"
        );
    }

    #[test]
    fn appending_never_changes_the_entry_that_plays_next() {
        for _ in 0..50 {
            let first = entries(30);
            let mut traversal = Traversal::default();
            traversal.toggle_shuffle(&first, Some(first[0].id));
            let all = first.clone();
            let before = traversal.next(&all, Some(0), Advance::Completed);
            let mut known = first.clone();
            for _ in 0..5 {
                let added = entries(20);
                known.extend(added.iter().copied());
                traversal.append(&added, Some(first[0].id));
            }
            assert_eq!(traversal.next(&known, Some(0), Advance::Completed), before);
        }
    }

    #[test]
    fn entries_appended_after_the_queue_ended_follow_the_played_ones() {
        for _ in 0..30 {
            let first = entries(6);
            let added = entries(6);
            let mut traversal = Traversal::default();
            traversal.toggle_shuffle(&first, Some(first[0].id));
            traversal.append(&added, None);
            let later = added.iter().map(|entry| entry.id).collect::<HashSet<_>>();
            let all = first.iter().chain(&added).copied().collect::<Vec<_>>();
            let mut current = 0;
            let mut seen = Vec::new();
            while let Some(next) = traversal.next(&all, Some(current), Advance::Completed) {
                current = all.iter().position(|entry| entry.id == next).unwrap();
                seen.push(next);
            }
            assert!(seen[5..].iter().all(|id| later.contains(id)));
        }
    }
}
