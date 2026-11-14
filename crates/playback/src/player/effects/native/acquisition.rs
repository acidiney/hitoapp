use super::super::{Key, Target};
use crate::{PlayerError, PlayerRetry, Status};
use std::time::Duration;
use std::{collections::HashMap, future::Future};
use tokio::time::Instant;
use uuid::Uuid;

pub(super) struct Acquisition {
    pub key: Key,
    pub demand: Uuid,
    pub started: Instant,
    pub deadline_at: Instant,
    pub attempts: u8,
    pub retrying: Option<PlayerRetry>,
    pub backoff_at: Option<Instant>,
    pub play_revision: u64,
    pub seek_id: Option<Uuid>,
    pub restoration_id: Option<Uuid>,
    baseline: Option<HeardBaseline>,
}

struct HeardBaseline {
    track: u64,
    position: Duration,
    seek_id: Option<Uuid>,
    restoration_id: Option<Uuid>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FinalizationId {
    pub key: Key,
    pub demand: Uuid,
    pub play_revision: u64,
    pub seek_id: Option<Uuid>,
    pub restoration_id: Option<Uuid>,
}

pub(super) struct Finalized {
    pub id: FinalizationId,
    pub drained: bool,
    pub access: std::result::Result<(), PlayerError>,
}

pub(super) struct FinalizationTasks {
    tasks: tokio::task::JoinSet<Finalized>,
    identities: HashMap<tokio::task::Id, FinalizationId>,
}

pub(super) enum FinalizationTaskResult {
    Finished(Finalized),
    Panicked(Option<FinalizationId>),
}

impl FinalizationTasks {
    pub fn new() -> Self {
        Self {
            tasks: tokio::task::JoinSet::new(),
            identities: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn spawn<F>(&mut self, id: FinalizationId, future: F)
    where
        F: Future<Output = Finalized> + Send + 'static,
    {
        let handle = self.tasks.spawn(future);
        self.identities.insert(handle.id(), id);
    }

    pub async fn join_next(&mut self) -> Option<FinalizationTaskResult> {
        self.tasks.join_next().await.map(|result| match result {
            Ok(result) => {
                self.identities.retain(|_, id| *id != result.id);
                FinalizationTaskResult::Finished(result)
            }
            Err(error) => FinalizationTaskResult::Panicked(self.identities.remove(&error.id())),
        })
    }
}

pub(super) fn failed_finalization_is_current(
    pending: Option<FinalizationId>,
    failed: Option<FinalizationId>,
    target: Option<Key>,
) -> bool {
    matches!((pending, failed, target), (Some(pending), Some(failed), Some(target))
        if pending == failed && pending.key == target)
}

impl Acquisition {
    pub fn start(
        key: Key,
        demand: Uuid,
        play_revision: u64,
        seek_id: Option<Uuid>,
        restoration_id: Option<Uuid>,
    ) -> Self {
        let started = Instant::now();
        let deadline_at = started + Duration::from_secs(15);
        tracing::info!(
            event = "player_acquisition_started",
            request_id = %demand,
            "Native playback acquisition started"
        );
        Self {
            key,
            demand,
            started,
            deadline_at,
            attempts: 1,
            retrying: None,
            backoff_at: None,
            play_revision,
            seek_id,
            restoration_id,
            baseline: None,
        }
    }

    pub fn finalization_id(&self) -> FinalizationId {
        FinalizationId {
            key: self.key,
            demand: self.demand,
            play_revision: self.play_revision,
            seek_id: self.seek_id,
            restoration_id: self.restoration_id,
        }
    }

    pub fn observe(
        &mut self,
        target: &Target,
        snapshot: crate::Snapshot,
        configured: bool,
        applied_seek: Option<Uuid>,
        restored: Option<(Uuid, Duration)>,
    ) -> bool {
        if self.key != target.key
            || !target.playing
            || !target.permitted
            || !target.output
            || self.play_revision != target.play_revision
            || self.seek_id != target.seek.map(|seek| seek.id)
            || self.restoration_id != target.restoration.map(|restoration| restoration.id)
            || !configured
            || snapshot.status != Status::Ready
            || !snapshot.playing
            || applied_seek != self.seek_id
            || restored.map(|restored| restored.0) != self.restoration_id
        {
            return false;
        }
        if let Some(baseline) = &self.baseline {
            return snapshot.track == baseline.track
                && snapshot.position > baseline.position
                && applied_seek == baseline.seek_id
                && restored.map(|restored| restored.0) == baseline.restoration_id;
        }
        self.baseline = Some(HeardBaseline {
            track: snapshot.track,
            position: restored.map_or(snapshot.position, |restored| {
                snapshot.position.max(restored.1)
            }),
            seek_id: applied_seek,
            restoration_id: restored.map(|restored| restored.0),
        });
        false
    }

    pub fn reset_baseline(&mut self) {
        self.baseline = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::effects::Key;

    fn finalization(selection: u64) -> FinalizationId {
        FinalizationId {
            key: Key {
                scope: Uuid::from_u128(10),
                selection,
                entry: Uuid::from_u128(11),
                start: None,
            },
            demand: Uuid::from_u128(selection as u128),
            play_revision: 2,
            seek_id: None,
            restoration_id: None,
        }
    }

    #[test]
    fn retired_finalizer_error_cannot_fail_successor_or_empty_target() {
        let retired = finalization(1);
        let successor = finalization(2);
        assert!(!failed_finalization_is_current(
            Some(successor),
            Some(retired),
            Some(successor.key)
        ));
        assert!(!failed_finalization_is_current(
            Some(retired),
            Some(retired),
            None
        ));
        assert!(failed_finalization_is_current(
            Some(successor),
            Some(successor),
            Some(successor.key)
        ));
    }

    #[tokio::test]
    async fn joinset_panic_keeps_the_identity_of_its_finalizer() {
        let old = finalization(1);
        let current = finalization(2);
        let (release_old, wait_old) = tokio::sync::oneshot::channel::<()>();
        let mut tasks = FinalizationTasks::new();
        tasks.spawn(old, async move {
            if wait_old.await.is_ok() {
                panic!("owned finalizer panic");
            }
            Finalized {
                id: old,
                drained: true,
                access: Ok(()),
            }
        });
        release_old.send(()).unwrap();
        let Some(FinalizationTaskResult::Panicked(Some(failed))) = tasks.join_next().await else {
            panic!("JoinSet must report the panicking task and its demand identity");
        };
        assert!(!failed_finalization_is_current(
            Some(current),
            Some(failed),
            Some(current.key)
        ));
        assert!(!failed_finalization_is_current(
            Some(old),
            Some(failed),
            None
        ));
        assert!(failed_finalization_is_current(
            Some(old),
            Some(failed),
            Some(old.key)
        ));
        assert!(tasks.is_empty());
    }
}
