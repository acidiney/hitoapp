use super::{
    PlayerError, PlayerPhase,
    attachment::Owned,
    effects::{Key, Update},
    ownership::State,
    progress::Progress,
};
use hito_core::{Collection, SavedPosition};
use std::{future::Future, pin::Pin};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(super) struct Restoration {
    pub id: Uuid,
    pub position_ms: u64,
}
enum Status {
    Waiting,
    Ready(u64),
    Failed,
    Superseded,
}
pub(super) struct SavedStart {
    pub id: Uuid,
    command_id: Uuid,
    status: Status,
    pub listened: bool,
}
impl SavedStart {
    pub fn new(command_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            command_id,
            status: Status::Waiting,
            listened: false,
        }
    }
    pub fn blocked(&self) -> bool {
        matches!(self.status, Status::Waiting | Status::Failed)
    }
    pub fn phase(&self) -> Option<(PlayerPhase, Option<PlayerError>)> {
        match self.status {
            Status::Waiting => Some((PlayerPhase::Loading, None)),
            Status::Failed => Some((PlayerPhase::Failed, Some(PlayerError::Resume))),
            _ => None,
        }
    }
    pub fn supersede(&mut self) {
        self.status = Status::Superseded;
    }
    pub fn restoration(&self) -> Option<Restoration> {
        match self.status {
            Status::Ready(position_ms) => Some(Restoration {
                id: self.id,
                position_ms,
            }),
            _ => None,
        }
    }
}
impl Owned {
    pub fn key(&self) -> Option<Key> {
        let snapshot = self.queue.snapshot();
        Some(Key {
            scope: self.scope,
            selection: snapshot.selection,
            entry: snapshot.current?.id,
            start: self.saved.as_ref().map(|saved| saved.id),
        })
    }
    pub fn listening(&self, update: &Update) -> bool {
        let Some(saved) = &self.saved else {
            return true;
        };
        match saved.status {
            Status::Waiting | Status::Failed => false,
            Status::Superseded => true,
            Status::Ready(_) => {
                saved.listened
                    || saved.restoration().zip(update.restored).is_some_and(
                        |(desired, restored)| {
                            restored.id == desired.id && update.position_ms > restored.position_ms
                        },
                    )
            }
        }
    }
}
pub(super) struct Report {
    key: Key,
    result: hito_core::Result<Option<SavedPosition>>,
}
type Pending = Pin<Box<dyn Future<Output = Report> + Send>>;
struct Load {
    key: Key,
    command_id: Uuid,
    watermark: u64,
    pending: Option<Pending>,
}
pub(super) struct Loader {
    collection: Collection,
    current: Option<Load>,
}
impl Loader {
    pub fn new(collection: Collection) -> Self {
        Self {
            collection,
            current: None,
        }
    }
    pub fn sync(&mut self, state: &State, progress: &mut Progress) {
        let demand = state.owner.as_ref().filter(|owned| {
            owned
                .saved
                .as_ref()
                .is_some_and(|saved| matches!(saved.status, Status::Waiting))
        });
        let key = demand.and_then(Owned::key);
        if self.current.as_ref().map(|load| load.key) != key {
            if let Some(load) = &self.current {
                tracing::debug!(event = "player_saved_load_canceled", command_id = %load.command_id, "Saved position load canceled");
            }
            self.current = key.map(|key| Load {
                key,
                command_id: demand.unwrap().saved.as_ref().unwrap().command_id,
                watermark: progress.barrier(),
                pending: None,
            });
        }
        let Some(load) = &mut self.current else {
            return;
        };
        if load.pending.is_none() && progress.settled(load.watermark) {
            let owned = demand.unwrap();
            let actor = owned.actor.clone();
            let recording = owned.queue.snapshot().current.unwrap().recording_id;
            let collection = self.collection.clone();
            let key = load.key;
            let command_id = load.command_id;
            load.pending = Some(Box::pin(async move {
                let started = std::time::Instant::now();
                tracing::debug!(event = "player_saved_load_started", command_id = %command_id, account_id = actor.user().id, "Loading saved playback position");
                let result = collection.saved_position(&actor, recording).await;
                tracing::debug!(event = "player_saved_load_finished", command_id = %command_id, success = result.is_ok(), elapsed_ms = started.elapsed().as_millis() as u64, "Saved playback position load finished");
                Report { key, result }
            }));
        }
    }
    pub async fn next(&mut self) -> Report {
        let report = match self.current.as_mut().and_then(|load| load.pending.as_mut()) {
            Some(pending) => pending.await,
            None => std::future::pending().await,
        };
        self.current = None;
        report
    }
}
impl State {
    pub fn loaded(&mut self, report: Report) {
        let Some(owned) = &mut self.owner else {
            return;
        };
        if owned.key() != Some(report.key) {
            return;
        }
        let Some(saved) = &mut owned.saved else {
            return;
        };
        if !matches!(saved.status, Status::Waiting) {
            return;
        }
        match report.result {
            Ok(position) => {
                saved.status = Status::Ready(position.map_or(0, |saved| saved.position_ms));
                if owned.authority_available {
                    owned.phase = PlayerPhase::Resolving;
                    owned.error = None;
                }
            }
            Err(hito_core::Error::Unauthorized) => {
                self.owner = None;
                self.epoch = Uuid::new_v4();
                return;
            }
            Err(error) => {
                tracing::debug!(event = "player_saved_load_failed", command_id = %saved.command_id, error_kind = error.code(), "Saved playback position could not be loaded");
                saved.status = Status::Failed;
                if owned.authority_available {
                    owned.phase = PlayerPhase::Failed;
                    owned.error = Some(PlayerError::Resume);
                }
            }
        }
        owned.publish();
    }
}
