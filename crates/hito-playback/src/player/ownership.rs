use super::attachment::Owned;
use super::{
    Action, PlayerAction, PlayerAttachment, PlayerError, PlayerInspection, PlayerPosition,
    PlayerReceipt, Response, Result,
};
use hito_core::{Actor, Recording};
use tokio::sync::watch;
use uuid::Uuid;

pub(super) struct State {
    pub epoch: Uuid,
    pub owner: Option<Owned>,
    pub native: bool,
}
impl State {
    pub fn preflight(&self, actor: &Actor, action: &Action) -> Result<Vec<Uuid>> {
        match action {
            Action::Submit(command) => {
                if self.owned(actor, command.lease)?.check(command)?.is_some() {
                    return Ok(Vec::new());
                }
                if matches!(
                    command.action,
                    PlayerAction::Resume {}
                        | PlayerAction::ResumeSaved {}
                        | PlayerAction::Pause {}
                        | PlayerAction::Seek { .. }
                        | PlayerAction::Volume { .. }
                        | PlayerAction::Speed { .. }
                        | PlayerAction::Previous {}
                        | PlayerAction::Repeat {}
                        | PlayerAction::Shuffle {}
                ) && !self.native
                {
                    return Err(PlayerError::Unsupported);
                }
                match &command.action {
                    super::PlayerAction::Replace { recordings, .. }
                    | super::PlayerAction::Append { recordings } => Ok(recordings.clone()),
                    _ => Ok(Vec::new()),
                }
            }
            Action::Queue {
                lease,
                revision,
                offset,
                limit,
            } => Ok(self
                .owned(actor, *lease)?
                .queue
                .references(*revision, *offset, *limit)?
                .iter()
                .map(|entry| entry.recording_id)
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    fn owned(&self, actor: &Actor, lease: Uuid) -> Result<&Owned> {
        self.owner
            .as_ref()
            .filter(|owner| owner.lease == lease && owner.actor.same_session(actor))
            .ok_or(PlayerError::Conflict)
    }

    pub fn accept(
        &mut self,
        actor: &Actor,
        action: Action,
        recordings: Vec<Recording>,
        progress_ready: bool,
    ) -> Result<Response> {
        match action {
            Action::Inspect => Ok(Response::Inspection(PlayerInspection {
                native: self.native,
                epoch: self.epoch,
                state: self
                    .owner
                    .as_ref()
                    .filter(|owner| owner.actor.same_session(actor))
                    .map(Owned::snapshot),
            })),
            Action::Claim(epoch) => {
                if epoch != self.epoch {
                    return Err(PlayerError::Conflict);
                }
                let mut owned = self
                    .owner
                    .take()
                    .filter(|owner| owner.actor.same_session(actor))
                    .unwrap_or_else(|| Owned::new(actor.clone()));
                self.epoch = Uuid::new_v4();
                let lease = Uuid::new_v4();
                let state = owned.snapshot();
                let response = Response::Attachment(PlayerAttachment {
                    native: self.native,
                    epoch: self.epoch,
                    lease,
                    state: state.clone(),
                });
                let (published, _) = watch::channel(state);
                let position = *owned.position.borrow();
                owned.position = watch::channel(position).0;
                owned.published = published;
                owned.lease = lease;
                owned.sequence = 0;
                owned.receipts.clear();
                self.owner = Some(owned);
                tracing::info!(
                    event = "player_claim_accepted",
                    "Playback attachment accepted"
                );
                Ok(response)
            }
            Action::Subscribe(lease) => Ok(Response::Observation(
                self.owned(actor, lease)?.published.subscribe(),
            )),
            Action::Position(lease) => Ok(Response::Position(
                self.owned(actor, lease)?.position.subscribe(),
            )),
            Action::Queue {
                lease,
                revision,
                offset,
                limit,
            } => Ok(Response::Queue(
                self.owned(actor, lease)?
                    .queue
                    .page(revision, offset, limit, recordings)?,
            )),
            Action::Submit(command) => {
                if let Some(receipt) = self.owned(actor, command.lease)?.check(&command)? {
                    return Ok(Response::Receipt(receipt));
                }
                if !self.owned(actor, command.lease)?.authority_available {
                    return Err(PlayerError::Unavailable);
                }
                if !progress_ready
                    && !matches!(
                        command.action,
                        PlayerAction::Pause {}
                            | PlayerAction::Clear {}
                            | PlayerAction::Volume { .. }
                    )
                {
                    return Err(PlayerError::Busy);
                }
                let owned = self.owner.as_mut().unwrap();
                let before = owned.queue.snapshot().selection;
                let position = owned
                    .seek
                    .filter(|seek| Some(seek.id) != owned.applied_seek)
                    .map(|seek| seek.position_ms)
                    .unwrap_or(owned.position.borrow().position_ms);
                let action = if matches!(command.action, PlayerAction::Previous {})
                    && owned.queue.snapshot().current.is_some()
                    && (position > 3000 || owned.queue.previous_entry().is_none())
                {
                    PlayerAction::Seek { position_ms: 0 }
                } else {
                    command.action.clone()
                };
                owned.queue.apply(action.clone())?;
                if matches!(
                    command.action,
                    PlayerAction::Resume {} | PlayerAction::ResumeSaved {}
                ) {
                    owned.desired_playing = owned.queue.snapshot().current.is_some();
                    owned.play_revision += 1;
                }
                if matches!(command.action, PlayerAction::ResumeSaved {}) {
                    owned.saved = Some(super::loading::SavedStart::new(command.id));
                    owned.seek = None;
                    owned.applied_seek = None;
                    owned.playing = false;
                    owned.phase = super::PlayerPhase::Loading;
                    owned.error = None;
                }
                if matches!(command.action, PlayerAction::Pause {}) {
                    owned.desired_playing = false;
                }
                if let PlayerAction::Volume { level } = command.action {
                    owned.volume = level;
                }
                if let PlayerAction::Speed { rate } = command.action {
                    owned.speed = rate;
                    tracing::debug!(
                        event = "player_speed_changed",
                        rate,
                        "Native playback speed requested"
                    );
                }
                if let PlayerAction::Seek { position_ms } = action {
                    if let Some(saved) = &mut owned.saved {
                        saved.supersede();
                    }
                    if owned.error == Some(PlayerError::Resume) {
                        owned.error = None;
                        owned.phase = super::PlayerPhase::Resolving;
                    }
                    owned.seek = Some(super::types::Seek {
                        id: command.id,
                        position_ms,
                    });
                }
                if before != owned.queue.snapshot().selection {
                    owned.selected(self.native);
                }
                if owned.queue.snapshot().current.is_none() {
                    owned.desired_playing = false;
                }
                owned.sequence = command.sequence;
                owned.publish();
                tracing::debug!(
                    event = "player_command_accepted",
                    revision = owned.queue.revision(),
                    "Queue command accepted"
                );
                let receipt = PlayerReceipt {
                    id: command.id,
                    sequence: command.sequence,
                    revision: owned.queue.revision(),
                };
                if owned.receipts.len() == 64 {
                    owned.receipts.pop_front();
                }
                owned.receipts.push_back((command, receipt.clone()));
                Ok(Response::Receipt(receipt))
            }
        }
    }
}

impl State {
    pub fn update(&mut self, update: super::effects::Update) {
        let Some(owned) = self.owner.as_mut() else {
            return;
        };
        let snapshot = owned.queue.snapshot();
        if owned.key() != Some(update.key) {
            return;
        }
        if update.error == Some(PlayerError::Unauthorized) {
            self.owner = None;
            self.epoch = Uuid::new_v4();
            return;
        }
        if !owned.authority_available {
            return;
        }
        if update.error == Some(PlayerError::Unavailable) {
            owned.authority(false, self.native);
            return;
        }
        if owned.saved.as_ref().is_some_and(|saved| saved.blocked()) {
            return;
        }
        let listening = owned.listening(&update);
        if listening && let Some(saved) = &mut owned.saved {
            saved.listened = true;
        }
        if listening
            && update.phase == super::PlayerPhase::Ready
            && update.error.is_none()
            && ((update.ended && owned.desired_playing)
                || (update.playing
                    && update.seek_id == owned.applied_seek
                    && update.position_ms > owned.position.borrow().position_ms))
            && update.seek_id == owned.seek.map(|seek| seek.id)
        {
            owned.queue.played();
        }
        if owned.phase != update.phase
            || owned.playing != update.playing
            || owned.error != update.error
            || owned.source_id != update.source_id
            || update.ended
        {
            tracing::debug!(
                event = "player_output_changed",
                account_id = owned.actor.user().id,
                selection = update.key.selection,
                phase = ?update.phase,
                playing = update.playing,
                ended = update.ended,
                error = ?update.error,
                "Native output changed"
            );
        }
        owned.position.send_if_modified(|position| {
            let next = PlayerPosition {
                entry: Some(update.key.entry),
                selection: update.key.selection,
                position_ms: update.position_ms,
                duration_ms: update.duration_ms,
                seek_id: update.seek_id,
            };
            if *position == next {
                false
            } else {
                *position = next;
                true
            }
        });
        owned.phase = update.phase;
        owned.playing = update.playing;
        owned.source_id = update.source_id;
        owned.error = update.error;
        owned.applied_seek = update.seek_id;
        if update.exhausted {
            owned.last_failure = snapshot.current.map(|entry| super::PlayerFailure {
                entry,
                error: update.error.unwrap_or(PlayerError::Playback),
            });
        }
        if update.ended || (update.exhausted && owned.desired_playing) {
            let advanced = owned.queue.finish(!update.ended);
            if advanced.is_err() {
                owned.error = Some(PlayerError::Invalid);
            }
            owned.selected(self.native);
            if owned.queue.snapshot().current.is_none() {
                owned.desired_playing = false;
            }
        }
        owned.publish();
    }

    #[cfg(feature = "device")]
    pub fn target(&self) -> Option<super::effects::Target> {
        let owned = self.owner.as_ref()?;
        let snapshot = owned.queue.snapshot();
        let current = snapshot.current?;
        Some(super::effects::Target {
            key: owned.key()?,
            actor: owned.actor.clone(),
            recording: current.recording_id,
            playing: owned.desired_playing,
            play_revision: owned.play_revision,
            seek: owned.seek,
            restoration: owned.saved.as_ref().and_then(|saved| saved.restoration()),
            output: owned.saved.as_ref().is_none_or(|saved| !saved.blocked()),
            volume: owned.volume,
            speed: owned.speed,
            permitted: owned.authority_available,
        })
    }
}
