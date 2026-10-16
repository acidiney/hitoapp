use super::{
    PlayerAction, PlayerEntryRef, PlayerError, PlayerQueueEntry, PlayerQueuePage, PlayerRepeat,
    PlayerSnapshot, Result,
};
use hito_core::Recording;
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct Queue {
    revision: u64,
    selection: u64,
    entries: Vec<PlayerEntryRef>,
    current: Option<usize>,
    history: VecDeque<Uuid>,
    repeat: PlayerRepeat,
}
impl Queue {
    pub fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            revision: self.revision,
            selection: self.selection,
            queue_len: self.entries.len(),
            current_index: self.current,
            can_previous: self.current.is_some() || !self.history.is_empty(),
            repeat: self.repeat,
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
            PlayerAction::Repeat {} => {
                self.repeat = match self.repeat {
                    PlayerRepeat::Off => PlayerRepeat::All,
                    PlayerRepeat::All => PlayerRepeat::One,
                    PlayerRepeat::One => PlayerRepeat::Off,
                };
                tracing::debug!(event = "player_repeat_changed", mode = ?self.repeat, "Repeat mode changed");
                return Ok(());
            }
            PlayerAction::Pause {} | PlayerAction::Volume { .. } => return Ok(()),
            PlayerAction::Seek { .. } => {
                return if self.current.is_some() {
                    Ok(())
                } else {
                    Err(PlayerError::Invalid)
                };
            }
            PlayerAction::Resume {} => {
                if self.current.is_some() || self.entries.is_empty() {
                    return Ok(());
                }
                self.current = Some(0);
            }
            PlayerAction::Clear {} => {
                self.entries.clear();
                self.history.clear();
                self.current = None;
            }
            PlayerAction::Select { entry } => self.current = Some(self.index(entry)?),
            PlayerAction::Append { recordings } => {
                if self.entries.len() + recordings.len() > 100_000 {
                    return Err(PlayerError::Invalid);
                }
                self.entries
                    .extend(recordings.into_iter().map(|recording_id| PlayerEntryRef {
                        id: Uuid::new_v4(),
                        recording_id,
                    }));
            }
            PlayerAction::Next {} => {
                self.current = self.current.and_then(|index| {
                    if index + 1 < self.entries.len() {
                        Some(index + 1)
                    } else {
                        (self.repeat == PlayerRepeat::All).then_some(0)
                    }
                });
            }
            PlayerAction::Previous {} => {
                let Some(entry) = self.previous_entry() else {
                    return Ok(());
                };
                self.current = Some(self.index(entry)?);
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
                self.entries.remove(index);
                self.history.retain(|played| *played != entry);
                self.current = self.current.and_then(|current| {
                    if current > index {
                        Some(current - 1)
                    } else if current < self.entries.len() {
                        Some(current)
                    } else {
                        None
                    }
                });
            }
        }
        if self.current_entry() != current || restart {
            self.selection = selection;
        }
        self.revision = revision;
        Ok(())
    }

    pub fn completed(&mut self) -> Result<()> {
        match (self.repeat, self.current_entry()) {
            (PlayerRepeat::One, Some(entry)) => self.apply(PlayerAction::Select { entry }),
            _ => self.apply(PlayerAction::Next {}),
        }
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
