use super::{
    PlayerAction, PlayerEntryRef, PlayerError, PlayerQueueEntry, PlayerQueuePage, PlayerSnapshot,
    Result,
};
use hito_core::Recording;
use std::collections::VecDeque;
use uuid::Uuid;
mod traversal;
use traversal::{Advance, Traversal};

#[derive(Default)]
pub(super) struct Queue {
    revision: u64,
    selection: u64,
    entries: Vec<PlayerEntryRef>,
    current: Option<usize>,
    history: VecDeque<Uuid>,
    traversal: Traversal,
}
impl Queue {
    pub fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            revision: self.revision,
            selection: self.selection,
            queue_len: self.entries.len(),
            current_index: self.current,
            can_previous: self.current.is_some() || !self.history.is_empty(),
            repeat: self.traversal.repeat,
            shuffle: self.traversal.shuffled(),
            current: self
                .current
                .and_then(|index| self.entries.get(index))
                .cloned(),
            ..Default::default()
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn previous_entry(&self) -> Option<Uuid> {
        self.history
            .iter()
            .rev()
            .find(|id| Some(**id) != self.current_entry())
            .copied()
    }

    pub fn played(&mut self) {
        self.traversal.reset_failures();
        if let Some(entry) = self.current_entry()
            && self.history.back() != Some(&entry)
        {
            if self.history.len() == 100_000 {
                self.history.pop_front();
            }
            self.history.push_back(entry);
            tracing::debug!(event = "player_entry_played", entry_id = %entry, retained = self.history.len(), "Played queue occurrence retained");
        }
    }

    pub fn apply(&mut self, action: PlayerAction) -> Result<()> {
        let revision = self.revision.checked_add(1).ok_or(PlayerError::Invalid)?;
        let selection = self.selection.checked_add(1).ok_or(PlayerError::Invalid)?;
        let current = self.current_entry();
        let restart = matches!(action, PlayerAction::Select { .. });
        match action {
            PlayerAction::Shuffle {} => {
                self.traversal.toggle_shuffle(&self.entries, current);
                return Ok(());
            }
            PlayerAction::Repeat {} => {
                self.traversal.cycle_repeat();
                return Ok(());
            }
            PlayerAction::Pause {} | PlayerAction::Volume { .. } => return Ok(()),
            PlayerAction::Seek { .. } => {
                self.traversal.reset_failures();
                return if self.current.is_some() {
                    Ok(())
                } else {
                    Err(PlayerError::Invalid)
                };
            }
            PlayerAction::Resume {} => {
                self.traversal.reset_failures();
                if self.current.is_some() || self.entries.is_empty() {
                    return Ok(());
                }
                self.current = self
                    .traversal
                    .first(&self.entries)
                    .and_then(|id| self.entries.iter().position(|entry| entry.id == id));
            }
            PlayerAction::Clear {} => {
                self.entries.clear();
                self.history.clear();
                self.current = None;
                self.traversal.replace(&self.entries, None);
            }
            PlayerAction::Select { entry } => {
                self.current = Some(self.index(entry)?);
                self.traversal.replace(&self.entries, Some(entry));
            }
            PlayerAction::Append { recordings } => {
                if self.entries.len() + recordings.len() > 100_000 {
                    return Err(PlayerError::Invalid);
                }
                let offset = self.entries.len();
                self.entries
                    .extend(recordings.into_iter().map(|recording_id| PlayerEntryRef {
                        id: Uuid::new_v4(),
                        recording_id,
                    }));
                self.traversal.append(&self.entries[offset..]);
            }
            PlayerAction::Next {} => return self.advance(Advance::Requested),
            PlayerAction::Previous {} => {
                let Some(entry) = self.previous_entry() else {
                    return Ok(());
                };
                self.current = Some(self.index(entry)?);
                self.traversal.reset_failures();
                while self.history.back() != Some(&entry) {
                    self.history.pop_back();
                }
            }
            PlayerAction::Replace { start, recordings } => {
                self.history.clear();
                self.entries = recordings
                    .into_iter()
                    .map(|recording_id| PlayerEntryRef {
                        id: Uuid::new_v4(),
                        recording_id,
                    })
                    .collect();
                self.current = Some(start);
                self.traversal.replace(&self.entries, self.current_entry());
            }
            PlayerAction::Move { entry, before } => {
                let from = self.index(entry)?;
                if let Some(before) = before {
                    self.index(before)?;
                }
                if before != Some(entry) {
                    let moved = self.entries.remove(from);
                    let to = before
                        .and_then(|id| self.entries.iter().position(|entry| entry.id == id))
                        .unwrap_or(self.entries.len());
                    self.entries.insert(to, moved);
                    self.current =
                        current.and_then(|id| self.entries.iter().position(|entry| entry.id == id));
                }
            }
            PlayerAction::Remove { entry } => {
                let index = self.index(entry)?;
                let next = if current == Some(entry) {
                    self.traversal
                        .next(&self.entries, current, Advance::Requested)
                        .filter(|id| *id != entry)
                } else {
                    current
                };
                self.entries.remove(index);
                self.history.retain(|played| *played != entry);
                self.traversal.remove(entry);
                self.current =
                    next.and_then(|id| self.entries.iter().position(|entry| entry.id == id));
            }
        }
        if self.current_entry() != current || restart {
            self.selection = selection;
        }
        self.revision = revision;
        Ok(())
    }

    pub fn finish(&mut self, failed: bool) -> Result<()> {
        self.advance(if failed {
            Advance::Failed
        } else {
            Advance::Completed
        })
    }

    fn advance(&mut self, reason: Advance) -> Result<()> {
        let revision = self.revision.checked_add(1).ok_or(PlayerError::Invalid)?;
        let selection = self.selection.checked_add(1).ok_or(PlayerError::Invalid)?;
        let current = self.current_entry();
        self.current = self
            .traversal
            .next(&self.entries, current, reason)
            .and_then(|id| self.entries.iter().position(|entry| entry.id == id));
        if current.is_some() || self.current.is_some() {
            self.selection = selection;
        }
        self.revision = revision;
        Ok(())
    }

    fn current_entry(&self) -> Option<Uuid> {
        self.current.map(|index| self.entries[index].id)
    }

    fn index(&self, id: Uuid) -> Result<usize> {
        self.entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(PlayerError::NotFound)
    }

    pub fn references(
        &self,
        revision: u64,
        offset: usize,
        limit: usize,
    ) -> Result<&[PlayerEntryRef]> {
        if revision != self.revision {
            return Err(PlayerError::Conflict);
        }
        if offset > self.entries.len() {
            return Err(PlayerError::Invalid);
        }
        Ok(&self.entries[offset..(offset + limit).min(self.entries.len())])
    }

    pub fn page(
        &self,
        revision: u64,
        offset: usize,
        limit: usize,
        recordings: Vec<Recording>,
    ) -> Result<PlayerQueuePage> {
        let entries = self.references(revision, offset, limit)?;
        if entries.len() != recordings.len() {
            return Err(PlayerError::Unavailable);
        }
        Ok(PlayerQueuePage {
            revision,
            total: self.entries.len(),
            offset,
            entries: entries
                .iter()
                .zip(recordings)
                .map(|(entry, recording)| PlayerQueueEntry {
                    id: entry.id,
                    recording,
                })
                .collect(),
        })
    }
}
