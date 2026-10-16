use super::{
    PlayerCommand, PlayerError, PlayerPhase, PlayerPosition, PlayerReceipt, PlayerSnapshot, Result,
    queue::Queue,
};
use hito_core::Actor;
use std::collections::VecDeque;
use tokio::sync::watch;
use uuid::Uuid;

pub(super) struct Owned {
    pub(super) actor: Actor,
    pub(super) authority_available: bool,
    pub(super) lease: Uuid,
    pub(super) queue: Queue,
    pub(super) sequence: u64,
    pub(super) receipts: VecDeque<(PlayerCommand, PlayerReceipt)>,
    pub(super) published: watch::Sender<PlayerSnapshot>,
    pub(super) scope: Uuid,
    pub(super) desired_playing: bool,
    pub(super) play_revision: u64,
    pub(super) playing: bool,
    pub(super) saved: Option<super::loading::SavedStart>,
    pub(super) seek: Option<super::types::Seek>,
    pub(super) applied_seek: Option<Uuid>,
    pub(super) volume: u16,
    pub(super) speed: u16,
    pub(super) source_id: Option<String>,
    pub(super) phase: PlayerPhase,
    pub(super) error: Option<PlayerError>,
    pub(super) last_failure: Option<super::PlayerFailure>,
    pub(super) persistence_error: Option<PlayerError>,
    pub(super) position: watch::Sender<PlayerPosition>,
}
impl Owned {
    pub(super) fn new(actor: Actor) -> Self {
        Self {
            actor,
            authority_available: true,
            lease: Uuid::new_v4(),
            queue: Queue::default(),
            sequence: 0,
            receipts: VecDeque::new(),
            published: watch::channel(PlayerSnapshot::default()).0,
            scope: Uuid::new_v4(),
            desired_playing: false,
            play_revision: 0,
            playing: false,
            saved: None,
            seek: None,
            applied_seek: None,
            volume: 1000,
            speed: 1000,
            source_id: None,
            phase: PlayerPhase::Idle,
            error: None,
            last_failure: None,
            persistence_error: None,
            position: watch::channel(PlayerPosition::default()).0,
        }
    }

    pub(super) fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            desired_playing: self.desired_playing,
            playing: self.playing,
            seeking: self.seek.map(|seek| seek.id) != self.applied_seek,
            volume: self.volume,
            speed: self.speed,
            source_id: self.source_id.clone(),
            phase: self.phase,
            error: self.error,
            last_failure: self.last_failure.clone(),
            persistence_error: self.persistence_error,
            ..self.queue.snapshot()
        }
    }
    pub(super) fn publish(&self) {
        let next = self.snapshot();
        self.published.send_if_modified(|previous| {
            if *previous == next {
                false
            } else {
                *previous = next;
                true
            }
        });
    }
    pub(super) fn authority(&mut self, available: bool, native: bool) {
        if self.authority_available == available {
            return;
        }
        self.authority_available = available;
        self.playing = false;
        self.source_id = None;
        self.phase = if !available {
            PlayerPhase::Suspended
        } else if native && self.queue.snapshot().current.is_some() {
            PlayerPhase::Resolving
        } else {
            PlayerPhase::Idle
        };
        self.error = (!available).then_some(PlayerError::Unavailable);
        if available
            && let Some((phase, error)) = self.saved.as_ref().and_then(|saved| saved.phase())
        {
            self.phase = phase;
            self.error = error;
        }
        self.publish();
        tracing::info!(
            event = if available {
                "player_authority_restored"
            } else {
                "player_authority_suspended"
            },
            account_id = self.actor.user().id,
            available,
            "Playback authority availability changed"
        );
    }
    pub(super) fn selected(&mut self, native: bool) {
        let snapshot = self.queue.snapshot();
        self.position.send_replace(PlayerPosition {
            entry: snapshot.current.map(|entry| entry.id),
            selection: snapshot.selection,
            ..Default::default()
        });
        self.phase = if native && snapshot.current.is_some() {
            PlayerPhase::Resolving
        } else {
            PlayerPhase::Idle
        };
        self.playing = false;
        self.source_id = None;
        self.error = None;
        self.saved = None;
        self.seek = None;
        self.applied_seek = None;
    }
    pub(super) fn check(&self, command: &PlayerCommand) -> Result<Option<PlayerReceipt>> {
        if let Some((accepted, receipt)) = self
            .receipts
            .iter()
            .find(|(accepted, _)| accepted.id == command.id)
        {
            return if accepted == command {
                Ok(Some(receipt.clone()))
            } else {
                Err(PlayerError::Conflict)
            };
        }
        if command.sequence <= self.sequence || command.revision != self.queue.revision() {
            return Err(PlayerError::Conflict);
        }
        Ok(None)
    }
}
