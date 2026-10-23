use crate::{
    Command, Error, Result, Snapshot, SourceStop, Status,
    session_worker::{OwnerStop, Request},
};
use hito_core::PlaybackSource;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, SyncSender},
};
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};
use tracing::{Instrument, instrument::WithSubscriber};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

pub struct Session {
    id: u64,
    grant: PlaybackSource,
    commands: SyncSender<Request>,
    cancel: Arc<OwnerStop>,
    state: watch::Receiver<Snapshot>,
    stop: Option<oneshot::Sender<()>>,
    owner: Option<JoinHandle<()>>,
}

impl Session {
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn open(grant: PlaybackSource) -> Result<Self> {
        let session = Self::start(grant);
        session.ready().await?;
        tracing::info!(
            event = "native_session_ready",
            "Authorized native playback owner ready"
        );
        Ok(session)
    }

    #[tracing::instrument(level = "info", skip_all, fields(native_session))]
    pub fn start(grant: PlaybackSource) -> Self {
        let id = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        tracing::Span::current().record("native_session", id);
        let cancel = Arc::new(OwnerStop::new(SourceStop::new()));
        let (commands, requests) = mpsc::sync_channel(8);
        let (state, snapshot) = watch::channel(Snapshot::starting());
        let (stop, stopped) = oneshot::channel();
        let owner = tokio::spawn(
            crate::session_owner::own(grant.clone(), requests, cancel.clone(), state, stopped)
                .instrument(tracing::Span::current())
                .with_current_subscriber(),
        );
        Self {
            id,
            grant,
            commands,
            cancel,
            state: snapshot,
            stop: Some(stop),
            owner: Some(owner),
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.state.clone()
    }

    pub async fn ready(&self) -> Result<()> {
        let mut state = self.state.clone();
        loop {
            let snapshot = *state.borrow_and_update();
            if matches!(
                snapshot.status,
                Status::Opening | Status::Starting | Status::Ready
            ) && self.cancel.requested()
            {
                return Err(Error::Stopped);
            }
            match snapshot.status {
                Status::Ready => return Ok(()),
                Status::Opening | Status::Starting => (),
                _ => return Err(snapshot.error.unwrap_or(Error::Stopped)),
            }
            state.changed().await.map_err(|_| Error::Worker)?;
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        *self.state.borrow()
    }

    #[tracing::instrument(level = "debug", skip_all, fields(native_session = self.id))]
    pub fn stop(&self) {
        self.cancel.request();
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(native_session = self.id))]
    pub async fn command(&self, command: Command) -> Result<()> {
        if self.cancel.requested() {
            return Err(Error::Stopped);
        }
        if self.grant.check().await.is_err() {
            self.cancel.request();
            return Err(Error::Access);
        }
        let snapshot = self.snapshot();
        match snapshot.status {
            Status::Ready => (),
            Status::Opening | Status::Starting => return Err(Error::Busy),
            _ => return Err(snapshot.error.unwrap_or(Error::Stopped)),
        }
        let (reply, received) = oneshot::channel();
        self.commands
            .try_send(Request { command, reply })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => Error::Busy,
                mpsc::TrySendError::Disconnected(_) => Error::Stopped,
            })?;
        received.await.map_err(|_| Error::Stopped)?
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(native_session = self.id))]
    pub async fn close(mut self) -> Result<()> {
        self.cancel.request();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(owner) = self.owner.take() {
            owner.await.map_err(|_| Error::Worker)?;
        }
        tracing::info!(
            event = "native_session_closed",
            "Native playback owner joined"
        );
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.request();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
