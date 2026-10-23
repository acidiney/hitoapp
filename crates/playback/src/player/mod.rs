mod attachment;
mod authority;
mod continuation;
mod continuation_commit;
mod effects;
mod loading;
mod owner;
mod ownership;
mod progress;
mod queue;
mod types;
use hito_core::Actor;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tracing::{Instrument, instrument::WithSubscriber};
pub use types::{
    PlayerAction, PlayerAttachment, PlayerAudiobookContinuation, PlayerAudiobookRequest,
    PlayerCommand, PlayerEntryRef, PlayerError, PlayerFailure, PlayerInspection, PlayerPhase,
    PlayerPosition, PlayerQueueEntry, PlayerQueueInput, PlayerQueuePage, PlayerReceipt,
    PlayerRepeat, PlayerSnapshot,
};
use uuid::Uuid;

type Result<T> = std::result::Result<T, PlayerError>;
static NEXT_PLAYER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct Player {
    id: u64,
    native: bool,
    requests: mpsc::Sender<Request>,
}

pub struct PlayerOwner {
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<()>>>,
}

enum Action {
    Inspect,
    Claim(Uuid),
    Subscribe(Uuid),
    Position(Uuid),
    Submit(PlayerCommand),
    PrepareAudiobook(PlayerAudiobookRequest),
    CancelAudiobook {
        lease: Uuid,
        id: Uuid,
        command: Option<PlayerCommand>,
    },
    Queue {
        lease: Uuid,
        revision: u64,
        offset: usize,
        limit: usize,
    },
}
enum Response {
    Inspection(PlayerInspection),
    Attachment(PlayerAttachment),
    Observation(watch::Receiver<PlayerSnapshot>),
    Position(watch::Receiver<PlayerPosition>),
    Receipt(PlayerReceipt),
    Queue(PlayerQueuePage),
    AudiobookContinuation(PlayerAudiobookContinuation),
    Cancelled,
    AudiobookSettlement(Option<PlayerReceipt>),
}
struct Request {
    actor: Actor,
    action: Action,
    reply: oneshot::Sender<Result<Response>>,
    span: tracing::Span,
    started: std::time::Instant,
}

impl Player {
    #[tracing::instrument(level = "info", skip_all, fields(player_owner))]
    pub fn start(services: hito_core::Services) -> (Self, PlayerOwner) {
        Self::spawn(services, effects::Effects::Disabled)
    }

    #[cfg(feature = "device")]
    #[tracing::instrument(level = "info", skip_all, fields(player_owner))]
    pub fn native(services: hito_core::Services) -> (Self, PlayerOwner) {
        let effects = effects::Effects::native(services.catalog.clone(), services.playback.clone());
        Self::spawn(services, effects)
    }

    fn spawn(services: hito_core::Services, effects: effects::Effects) -> (Self, PlayerOwner) {
        let native = effects.enabled();
        let id = NEXT_PLAYER.fetch_add(1, Ordering::Relaxed);
        tracing::Span::current().record("player_owner", id);
        let (requests, received) = mpsc::channel(32);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(
            owner::run(services, received, stopped, effects)
                .instrument(tracing::Span::current())
                .with_current_subscriber(),
        );
        (
            Self {
                id,
                native,
                requests,
            },
            PlayerOwner {
                stop: Some(stop),
                task: Some(task),
            },
        )
    }

    pub fn native_enabled(&self) -> bool {
        self.native
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id, request_id = %input.id))]
    pub async fn prepare_audiobook(
        &self,
        actor: &Actor,
        input: PlayerAudiobookRequest,
    ) -> Result<PlayerAudiobookContinuation> {
        if input.id.is_nil() || input.lease.is_nil() || input.audiobook_id.is_nil() {
            return Err(PlayerError::Invalid);
        }
        match self.request(actor, Action::PrepareAudiobook(input)).await? {
            Response::AudiobookContinuation(value) => Ok(value),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id, request_id = %id))]
    pub async fn cancel_audiobook(&self, actor: &Actor, lease: Uuid, id: Uuid) -> Result<()> {
        if lease.is_nil() || id.is_nil() {
            return Err(PlayerError::Invalid);
        }
        match self
            .request(
                actor,
                Action::CancelAudiobook {
                    lease,
                    id,
                    command: None,
                },
            )
            .await?
        {
            Response::Cancelled => Ok(()),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id, request_id = %id))]
    pub async fn settle_audiobook(
        &self,
        actor: &Actor,
        lease: Uuid,
        id: Uuid,
        command: PlayerCommand,
    ) -> Result<Option<PlayerReceipt>> {
        if lease.is_nil()
            || id.is_nil()
            || command.lease != lease
            || command.id.is_nil()
            || !matches!(command.action, PlayerAction::ContinueAudiobook { token } if !token.is_nil())
        {
            return Err(PlayerError::Invalid);
        }
        match self
            .request(
                actor,
                Action::CancelAudiobook {
                    lease,
                    id,
                    command: Some(command),
                },
            )
            .await?
        {
            Response::AudiobookSettlement(receipt) => Ok(receipt),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id))]
    pub async fn inspect(&self, actor: &Actor) -> Result<PlayerInspection> {
        match self.request(actor, Action::Inspect).await? {
            Response::Inspection(value) => Ok(value),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id))]
    pub async fn claim(&self, actor: &Actor, epoch: Uuid) -> Result<PlayerAttachment> {
        match self.request(actor, Action::Claim(epoch)).await? {
            Response::Attachment(value) => Ok(value),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id))]
    pub async fn subscribe(
        &self,
        actor: &Actor,
        lease: Uuid,
    ) -> Result<watch::Receiver<PlayerSnapshot>> {
        match self.request(actor, Action::Subscribe(lease)).await? {
            Response::Observation(value) => Ok(value),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id))]
    pub async fn subscribe_position(
        &self,
        actor: &Actor,
        lease: Uuid,
    ) -> Result<watch::Receiver<PlayerPosition>> {
        match self.request(actor, Action::Position(lease)).await? {
            Response::Position(value) => Ok(value),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id, command_id = %command.id, sequence = command.sequence, action = command.action.name()))]
    pub async fn submit(&self, actor: &Actor, command: PlayerCommand) -> Result<PlayerReceipt> {
        match &command.action {
            PlayerAction::Volume { level } if *level > 1000 => {
                return Err(PlayerError::Invalid);
            }
            PlayerAction::Speed { rate } if !(500..=3000).contains(rate) => {
                return Err(PlayerError::Invalid);
            }
            PlayerAction::Seek { position_ms } if *position_ms > 9_007_199_254_740_991 => {
                return Err(PlayerError::Invalid);
            }
            PlayerAction::ContinueAudiobook { token } if token.is_nil() => {
                return Err(PlayerError::Invalid);
            }
            PlayerAction::Append { recordings }
                if recordings.is_empty() || recordings.len() > 1000 =>
            {
                return Err(PlayerError::Invalid);
            }
            PlayerAction::Replace { recordings, start }
                if recordings.is_empty()
                    || recordings.len() > 1000
                    || *start >= recordings.len() =>
            {
                return Err(PlayerError::Invalid);
            }
            PlayerAction::AppendEntries { entries }
                if entries.is_empty()
                    || entries.len() > 1000
                    || entries.iter().any(|entry| !entry.valid()) =>
            {
                return Err(PlayerError::Invalid);
            }
            PlayerAction::ReplaceEntries { entries, start }
                if entries.is_empty()
                    || entries.len() > 1000
                    || *start >= entries.len()
                    || entries.iter().any(|entry| !entry.valid()) =>
            {
                return Err(PlayerError::Invalid);
            }
            _ => (),
        }
        if command.sequence == 0 || command.sequence > 9_007_199_254_740_991 {
            return Err(PlayerError::Invalid);
        }
        match self.request(actor, Action::Submit(command)).await? {
            Response::Receipt(value) => Ok(value),
            _ => Err(PlayerError::Closed),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(player_owner = self.id, account_id = actor.user().id, offset, limit))]
    pub async fn queue(
        &self,
        actor: &Actor,
        lease: Uuid,
        revision: u64,
        offset: usize,
        limit: usize,
    ) -> Result<PlayerQueuePage> {
        if limit == 0 || limit > 1000 {
            return Err(PlayerError::Invalid);
        }
        match self
            .request(
                actor,
                Action::Queue {
                    lease,
                    revision,
                    offset,
                    limit,
                },
            )
            .await?
        {
            Response::Queue(value) => Ok(value),
            _ => Err(PlayerError::Closed),
        }
    }

    async fn request(&self, actor: &Actor, action: Action) -> Result<Response> {
        let (reply, received) = oneshot::channel();
        self.requests
            .try_send(Request {
                actor: actor.clone(),
                action,
                reply,
                span: tracing::Span::current(),
                started: std::time::Instant::now(),
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PlayerError::Busy,
                mpsc::error::TrySendError::Closed(_) => PlayerError::Closed,
            })?;
        received.await.map_err(|_| PlayerError::Closed)?
    }
}

impl PlayerOwner {
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn close(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.await.map_err(|_| PlayerError::Closed)??;
        }
        Ok(())
    }
}
impl Drop for PlayerOwner {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
