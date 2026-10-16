use super::{
    Action, PlayerAttachment, PlayerCommand, PlayerError, PlayerInspection, PlayerReceipt,
    PlayerSnapshot, Response, Result, queue::Queue,
};
use hito_core::{Actor, Recording};
use std::collections::VecDeque;
use tokio::sync::watch;
use uuid::Uuid;

pub(super) struct State {
    pub epoch: Uuid,
    pub owner: Option<Owned>,
}
pub(super) struct Owned {
    pub actor: Actor,
    pub lease: Uuid,
    queue: Queue,
    sequence: u64,
    receipts: VecDeque<(PlayerCommand, PlayerReceipt)>,
    published: watch::Sender<PlayerSnapshot>,
}
impl State {
    pub fn preflight(&self, actor: &Actor, action: &Action) -> Result<Vec<Uuid>> {
        match action {
            Action::Submit(command) => {
                if self.owned(actor, command.lease)?.check(command)?.is_some() {
                    return Ok(Vec::new());
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
    ) -> Result<Response> {
        match action {
            Action::Inspect => Ok(Response::Inspection(PlayerInspection {
                epoch: self.epoch,
                state: self
                    .owner
                    .as_ref()
                    .filter(|owner| owner.actor.same_session(actor))
                    .map(|owner| owner.queue.snapshot()),
            })),
            Action::Claim(epoch) => {
                if epoch != self.epoch {
                    return Err(PlayerError::Conflict);
                }
                let queue = self
                    .owner
                    .take()
                    .filter(|owner| owner.actor.same_session(actor))
                    .map(|owner| owner.queue)
                    .unwrap_or_default();
                self.epoch = Uuid::new_v4();
                let lease = Uuid::new_v4();
                let state = queue.snapshot();
                let response = Response::Attachment(PlayerAttachment {
                    epoch: self.epoch,
                    lease,
                    state: state.clone(),
                });
                let (published, _) = watch::channel(state);
                self.owner = Some(Owned {
                    actor: actor.clone(),
                    lease,
                    queue,
                    sequence: 0,
                    receipts: VecDeque::new(),
                    published,
                });
                tracing::info!(
                    event = "player_claim_accepted",
                    "Playback attachment accepted"
                );
                Ok(response)
            }
            Action::Subscribe(lease) => Ok(Response::Observation(
                self.owned(actor, lease)?.published.subscribe(),
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
                let owned = self.owner.as_mut().unwrap();
                owned.queue.apply(command.action.clone())?;
                owned.sequence = command.sequence;
                owned.published.send_replace(owned.queue.snapshot());
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
impl Owned {
    fn check(&self, command: &PlayerCommand) -> Result<Option<PlayerReceipt>> {
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
