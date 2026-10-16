use super::{
    PlayerAction, PlayerEntryRef, PlayerError, PlayerQueueEntry, PlayerQueuePage, PlayerSnapshot,
    Result,
};
use hito_core::Recording;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct Queue {
    revision: u64,
    selection: u64,
    entries: Vec<PlayerEntryRef>,
    current: Option<usize>,
}
impl Queue {
    pub fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            revision: self.revision,
            selection: self.selection,
            queue_len: self.entries.len(),
            current_index: self.current,
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

    pub fn apply(&mut self, action: PlayerAction) -> Result<()> {
        let revision = self.revision.checked_add(1).ok_or(PlayerError::Invalid)?;
        let selection = self.selection.checked_add(1).ok_or(PlayerError::Invalid)?;
        let current = self.current_entry();
        let restart = matches!(action, PlayerAction::Select { .. });
        match action {
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
                self.current = self
                    .current
                    .filter(|index| *index + 1 < self.entries.len())
                    .map(|index| index + 1)
            }
            PlayerAction::Replace { start, recordings } => {
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
