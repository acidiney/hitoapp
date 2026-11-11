mod file;
pub(super) mod owner;
pub(super) mod registry;
#[cfg(test)]
mod testing;
mod types;

use super::{AudioRequest, OpenedAudio, PlaybackAccess, PlaybackSource};
use crate::{Actor, Error, Result};
use futures_util::FutureExt;
use registry::{Completion, Retirement, Work};
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tracing::{Instrument, instrument::WithSubscriber};
pub use types::{BeginPlaybackLease, PlaybackLease};
use uuid::Uuid;

static NEXT_LEASE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

const LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

struct Entry {
    #[cfg(test)]
    read_pause: OnceLock<Arc<testing::ReadPause>>,
    #[cfg(test)]
    bytes_read: std::sync::atomic::AtomicU64,
    #[cfg(test)]
    publications: testing::Publications,
    correlation: u64,
    started: std::time::Instant,
    actor: Actor,
    metadata: PlaybackLease,
    deadline: tokio::time::Instant,
    cancel: watch::Sender<bool>,
    ready: AtomicBool,
    grant: OnceLock<PlaybackSource>,
    commands: mpsc::Sender<owner::Command>,
    wake: Arc<Notify>,
}
impl Entry {
    fn stop(&self) {
        self.cancel.send_replace(true);
    }
    fn alive(&self) -> Result<()> {
        if *self.cancel.borrow() || tokio::time::Instant::now() >= self.deadline {
            Err(Error::NotFound)
        } else {
            Ok(())
        }
    }
    async fn check(&self) -> Result<()> {
        self.alive()?;
        if let Err(error) = self.grant.get().ok_or(Error::NotFound)?.check().await {
            self.stop();
            return Err(error);
        }
        self.alive()
    }
}

struct Observer(Option<Arc<Entry>>);
impl Drop for Observer {
    fn drop(&mut self) {
        if let Some(entry) = &self.0 {
            entry.stop();
        }
    }
}

impl PlaybackAccess {
    /// Retains exact session and source authority for at most 24 hours without renewal.
    /// Limits are eight leases and five pending creations per account, 32 and eight globally.
    /// Each lease admits two audio bodies. Indexed originals are retained without copying or hashing.
    /// Managed sources keep their existing verification policy.
    #[tracing::instrument(level="debug", skip_all, err(Debug), fields(account_id=actor.id(), playback_lease=tracing::field::Empty))]
    pub async fn begin_lease(
        &self,
        actor: &Actor,
        request: BeginPlaybackLease,
    ) -> Result<PlaybackLease> {
        actor.require_foreground_session()?;
        actor.require_unlocked()?;
        if [&request.recording_id, &request.source_id]
            .iter()
            .any(|id| id.is_empty() || id.len() > 1024)
        {
            return Err(Error::Invalid(
                "Use recording and source identities of 1 to 1024 bytes".into(),
            ));
        }
        #[cfg(test)]
        let lifetime = self
            .leases
            .state
            .lock()
            .map_err(|_| stopped())?
            .lifetime
            .unwrap_or(LIFETIME);
        #[cfg(not(test))]
        let lifetime = LIFETIME;
        let admission =
            PlaybackSource::admit(self, actor, &request.recording_id, &request.source_id);
        let (commands, receiver) = mpsc::channel(8);
        let entry = Arc::new(Entry {
            #[cfg(test)]
            read_pause: OnceLock::new(),
            #[cfg(test)]
            bytes_read: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            publications: testing::Publications::default(),
            correlation: NEXT_LEASE.fetch_add(1, Ordering::Relaxed),
            started: std::time::Instant::now(),
            actor: actor.clone(),
            metadata: PlaybackLease {
                id: Uuid::new_v4(),
                expires_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| stopped())?
                    .as_secs() as i64
                    + lifetime.as_secs().max(1) as i64,
            },
            deadline: tokio::time::Instant::now() + lifetime,
            cancel: watch::channel(false).0,
            ready: AtomicBool::new(false),
            grant: OnceLock::new(),
            commands,
            wake: Arc::new(Notify::new()),
        });
        let (reply, created) = oneshot::channel();
        {
            let mut state = self.leases.state.lock().map_err(|_| stopped())?;
            state.reserve(actor, entry.clone())?;
            #[cfg(test)]
            let admission_pause = state.admission_pause.clone();
            #[cfg(test)]
            let publication_pause = state.publication_pause.clone();
            #[cfg(test)]
            if let Some(pause) = &state.read_pause {
                let _ = entry.read_pause.set(pause.clone());
            }
            let owned = entry.clone();
            let weak = Arc::downgrade(&self.leases);
            let retirement = Retirement {
                entry: owned.clone(),
                registry: weak,
            };
            let task = tokio::spawn(
                async move {
                    let _retirement = retirement;
                    let result = async {
                        owned.alive()?;
                        #[cfg(test)]
                        if let Some(pause) = admission_pause {
                            pause.pause(&owned).await?;
                        }
                        let mut stop = owned.cancel.subscribe();
                        owned.alive()?;
                        let grant = tokio::select! {
                            biased;
                            _ = stop.changed() => Err(Error::NotFound),
                            result = tokio::time::timeout(Duration::from_secs(1), admission) => {
                                result.unwrap_or(Err(Error::Upstream("Playback authorization could not be checked in time")))
                            },
                        }?;
                        owned.alive()?;
                        let file = if grant.is_local() {
                            Some(Arc::new(file::RetainedFile::new(grant.open_file().await?)?))
                        } else {
                            None
                        };
                        owned.alive()?;
                        grant.check().await?;
                        #[cfg(test)]
                        if let Some(pause) = publication_pause {
                            pause.pause(&owned).await?;
                        }
                        let _ = owned.grant.set(grant);
                        Ok(file)
                    }
                    .await;
                    match result {
                        Ok(file) if owned.alive().is_ok() && !reply.is_closed() => {
                            owned.ready.store(true, Ordering::Release);
                            if reply.send(Ok(owned.metadata.clone())).is_ok() {
                                #[cfg(test)]
                                owned.publications.created.notify_one();
                                owner::serve(&owned, receiver, file).await;
                            }
                        }
                        Ok(_) => {
                            let _ = reply.send(Err(Error::NotFound));
                        }
                        Err(error) => {
                            let _ = reply.send(Err(error));
                        }
                    }
                    owned.stop();
                    Ok(())
                }
                .instrument(tracing::Span::current())
                .with_current_subscriber(),
            );
            let finished = task.abort_handle();
            let completion: Completion =
                async move { task.await.map_err(|_| Arc::new(stopped()))? }
                    .boxed()
                    .shared();
            state.work.insert(
                entry.metadata.id,
                Work {
                    owner: actor.clone(),
                    correlation: entry.correlation,
                    finished,
                    completion,
                },
            );
        }
        tracing::Span::current().record("playback_lease", entry.correlation);
        let started = entry.started;
        let mut observer = Observer(Some(entry));
        let lease = created.await.map_err(|_| stopped())??;
        observer.0.as_ref().ok_or_else(stopped)?.check().await?;
        observer.0 = None;
        tracing::debug!(
            event = "playback_lease_created",
            outcome = "created",
            elapsed_ms = started.elapsed().as_millis() as u64,
            lifetime_seconds = lifetime.as_secs(),
            "Playback lease created"
        );
        Ok(lease)
    }

    #[tracing::instrument(level="debug", skip_all, err(Debug), fields(account_id=actor.id(), playback_lease=tracing::field::Empty))]
    pub async fn lease_status(&self, actor: &Actor, id: Uuid) -> Result<PlaybackLease> {
        self.lease_actor(actor, id).await?;
        let entry = self.leases.entry(actor, id)?;
        tracing::Span::current().record("playback_lease", entry.correlation);
        entry.check().await?;
        Ok(entry.metadata.clone())
    }

    #[tracing::instrument(level="debug", skip_all, err(Debug), fields(account_id=actor.id(), playback_lease=tracing::field::Empty))]
    pub async fn open_lease_audio(
        &self,
        actor: &Actor,
        id: Uuid,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        self.lease_actor(actor, id).await?;
        let entry = self.leases.entry(actor, id)?;
        tracing::Span::current().record("playback_lease", entry.correlation);
        entry.check().await?;
        let (reply, opened) = oneshot::channel();
        entry
            .commands
            .send(owner::Command::Open(request, reply))
            .await
            .map_err(|_| Error::NotFound)?;
        let result = opened.await.map_err(|_| Error::NotFound)?;
        entry.check().await?;
        result
    }

    #[tracing::instrument(level="debug", skip_all, err(Debug), fields(account_id=actor.id(), playback_lease=tracing::field::Empty))]
    pub async fn release_lease(&self, actor: &Actor, id: Uuid) -> Result<()> {
        self.lease_actor(actor, id).await?;
        let completion = {
            let state = self.leases.state.lock().map_err(|_| stopped())?;
            if let Some(work) = state.work.get(&id) {
                if !actor.same_session(&work.owner) {
                    return Err(Error::NotFound);
                }
                tracing::Span::current().record("playback_lease", work.correlation);
            }
            if let Some(entry) = state.entries.get(&id) {
                if !actor.same_session(&entry.actor) {
                    return Err(Error::NotFound);
                }
                entry.stop();
            }
            state.work.get(&id).map(|work| work.completion.clone())
        };
        if let Some(completion) = completion {
            completion.await.map_err(|_| stopped())?;
        }
        Ok(())
    }

    /// Retires every transient lease and joins retained owners, including cancelled reads.
    /// Repeated calls observe the same cleanup even if an earlier caller was cancelled.
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn close(&self) -> Result<()> {
        let started = std::time::Instant::now();
        self.leases.closing()?.await.map_err(|_| stopped())?;
        tracing::debug!(
            event = "playback_leases_closed",
            outcome = "closed",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playback lease owners joined"
        );
        Ok(())
    }

    async fn lease_actor(&self, actor: &Actor, id: Uuid) -> Result<()> {
        actor.require_foreground_session()?;
        let authorization = match actor.require_unlocked() {
            Ok(()) => {
                tokio::time::timeout(Duration::from_secs(1), actor.check_session(&self.store))
                    .await
                    .unwrap_or(Err(Error::Upstream(
                        "Playback authorization could not be checked in time",
                    )))
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &authorization
            && let Ok(entry) = self.leases.entry(actor, id)
        {
            entry.stop();
            tracing::Span::current().record("playback_lease", entry.correlation);
            tracing::debug!(
                event = "playback_lease_authorization_denied",
                outcome = "retired",
                error_kind = error.code(),
                "The owning session's authorization failure retired its playback lease"
            );
        }
        authorization
    }
}

fn stopped() -> Error {
    Error::Conflict("Playback lease owner has closed or stopped".into())
}

#[cfg(test)]
#[path = "../../tests/playback_leases/work.rs"]
mod work;

#[cfg(test)]
#[path = "../../tests/playback_leases/publication.rs"]
mod publication;

#[cfg(test)]
#[path = "../../tests/playback_leases/pin_retirement.rs"]
mod pin_retirement;
