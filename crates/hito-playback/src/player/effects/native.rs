use super::{Key, Update, sources::Sources};
use crate::{PlayerError, PlayerPhase, Session, Status};
use hito_core::{Actor, Catalog};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub(crate) struct Target {
    pub key: Key,
    pub actor: Actor,
    pub recording: Uuid,
    pub playing: bool,
    pub play_revision: u64,
    pub seek: Option<super::super::types::Seek>,
    pub volume: u16,
}
struct Applied {
    seek_id: Option<Uuid>,
    volume: u16,
}
pub(crate) struct Native {
    sources: Sources,
    target: Option<Target>,
    session: Option<Arc<Session>>,
    observed: Option<watch::Receiver<crate::Snapshot>>,
    control: Option<Pending<crate::Result<Applied>>>,
    closing: Option<Pending<()>>,
    failed: bool,
    update: Option<Update>,
    resume_at: Duration,
    duration: Option<Duration>,
    configured: bool,
    applied_seek: Option<Uuid>,
    applied_volume: Option<u16>,
}
impl Native {
    pub fn new(catalog: Catalog) -> Self {
        Self {
            sources: Sources::new(catalog),
            target: None,
            session: None,
            observed: None,
            control: None,
            closing: None,
            failed: false,
            update: None,
            resume_at: Duration::ZERO,
            duration: None,
            configured: false,
            applied_seek: None,
            applied_volume: None,
        }
    }
    pub fn sync(&mut self, target: Option<Target>) {
        if self.target.as_ref().map(|target| target.key) != target.as_ref().map(|target| target.key)
        {
            self.retire();
            self.sources.reset(target.as_ref().map(|target| target.key));
            self.failed = false;
            self.resume_at = Duration::ZERO;
            self.duration = None;
            self.applied_seek = None;
        }
        if let Some(seek) = target.as_ref().and_then(|target| target.seek)
            && self.target.as_ref().and_then(|target| target.seek) != Some(seek)
        {
            if self.control.is_some() {
                self.retire();
            }
            self.resume_at = Duration::from_millis(seek.position_ms);
            self.configured = false;
        } else if target.as_ref().is_some_and(|next| {
            self.target
                .as_ref()
                .is_some_and(|old| old.volume != next.volume)
        }) && self.control.is_some()
        {
            self.retire();
        } else if target.as_ref().is_some_and(|next| {
            !next.playing && self.target.as_ref().is_some_and(|old| old.playing)
        }) && self.control.is_some()
        {
            self.retire();
            self.failed = false;
        } else if target.as_ref().is_some_and(|next| {
            next.playing
                && self
                    .target
                    .as_ref()
                    .is_some_and(|old| old.play_revision != next.play_revision)
        }) {
            if self.failed {
                self.sources.reset(target.as_ref().map(|target| target.key));
            }
            self.failed = false;
        }
        self.target = target;
        let Some(target) = self.target.as_ref() else {
            return;
        };
        if self.closing.is_none() && self.session.is_none() && !self.failed {
            if let Some(grant) = self.sources.current() {
                let session = Arc::new(Session::start(grant.clone()));
                self.observed = Some(session.subscribe());
                self.session = Some(session);
                self.configured = false;
                self.applied_volume = None;
                self.update = Some(self.status(PlayerPhase::Opening, None));
                return;
            }
            if self
                .sources
                .request(&target.actor, &target.recording.to_string())
            {
                self.update = Some(self.status(PlayerPhase::Resolving, None));
            }
        }
        if let Some(session) = &self.session {
            let snapshot = session.snapshot();
            if snapshot.status == Status::Ready
                && !snapshot.ended
                && self.control.is_none()
                && (!self.configured
                    || snapshot.playing != target.playing
                    || self.applied_volume != Some(target.volume))
                && !self.failed
            {
                let session = session.clone();
                let seek_id = target.seek.map(|seek| seek.id);
                let volume = target.volume;
                let seek = if self.configured
                    || (self.resume_at.is_zero() && seek_id == self.applied_seek)
                {
                    None
                } else {
                    Some(self.resume_at)
                };
                let command = if target.playing {
                    crate::Command::Resume
                } else {
                    crate::Command::Pause
                };
                self.control = Some(Box::pin(async move {
                    session
                        .command(crate::Command::Volume(f32::from(volume) / 1000.))
                        .await?;
                    if let Some(position) = seek {
                        session.command(crate::Command::Seek(position)).await?;
                    }
                    session.command(command).await?;
                    Ok(Applied { seek_id, volume })
                }));
            }
        }
    }
    pub async fn next(&mut self) -> Update {
        if let Some(update) = self.update.take() {
            return update;
        }
        loop {
            tokio::select! {
                biased;
                _ = async { self.closing.as_mut().unwrap().await }, if self.closing.is_some() => {
                    self.closing = None;
                    if self.target.is_some() {
                        return if self.failed { self.status(PlayerPhase::Failed, Some(PlayerError::Playback)) } else { self.status(PlayerPhase::Resolving, None) };
                    }
                },
                grant = self.sources.next() => {
                    match grant {
                        Ok(Some(_)) => return self.status(PlayerPhase::Resolving, None),
                        Ok(None) => return self.exhausted(PlayerError::Playback),
                        Err(error) => return self.exhausted(error),
                    }
                },
                result = async { self.control.as_mut().unwrap().await }, if self.control.is_some() => {
                    self.control = None;
                    let Ok(applied) = result else { return self.recover(); };
                    self.applied_seek = applied.seek_id;
                    self.applied_volume = Some(applied.volume);
                    self.configured = true;
                    return self.snapshot();
                },
                changed = async { self.observed.as_mut().unwrap().changed().await }, if self.observed.is_some() => {
                    if changed.is_err() { return self.recover(); }
                    let update = self.snapshot();
                    if update.phase == PlayerPhase::Failed { return self.recover(); }
                    return update;
                },
                else => std::future::pending().await,
            }
        }
    }
    fn snapshot(&self) -> Update {
        let snapshot = self.session.as_ref().unwrap().snapshot();
        let phase = match snapshot.status {
            Status::Ready => PlayerPhase::Ready,
            Status::Opening | Status::Starting => PlayerPhase::Opening,
            _ => PlayerPhase::Failed,
        };
        if !self.configured {
            return if phase == PlayerPhase::Failed {
                self.status(phase, Some(PlayerError::Playback))
            } else {
                self.status(PlayerPhase::Opening, None)
            };
        }
        Update {
            key: self.target.as_ref().unwrap().key,
            phase,
            playing: snapshot.playing,
            source_id: self
                .sources
                .current()
                .map(|grant| grant.source_id().to_owned()),
            position_ms: snapshot.position.as_millis().min(u64::MAX as u128) as u64,
            duration_ms: snapshot
                .format
                .and_then(|format| format.duration)
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64),
            seek_id: self.applied_seek,
            ended: snapshot.ended,
            exhausted: false,
            error: if phase == PlayerPhase::Failed {
                Some(PlayerError::Playback)
            } else {
                None
            },
        }
    }
    fn status(&self, phase: PlayerPhase, error: Option<PlayerError>) -> Update {
        Update {
            key: self.target.as_ref().unwrap().key,
            phase,
            playing: false,
            source_id: self
                .sources
                .current()
                .map(|grant| grant.source_id().to_owned()),
            position_ms: self.resume_at.as_millis().min(u64::MAX as u128) as u64,
            duration_ms: self
                .duration
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64),
            seek_id: self.applied_seek,
            ended: false,
            exhausted: false,
            error,
        }
    }
    fn retire(&mut self) {
        if let Some(session) = self.session.as_ref() {
            let snapshot = session.snapshot();
            if self.configured {
                self.resume_at = snapshot.position;
            }
            self.duration = snapshot.format.and_then(|format| format.duration);
            session.stop();
        }
        self.control = None;
        self.observed = None;
        if let Some(session) = self.session.take() {
            let session =
                Arc::into_inner(session).expect("native control reference survived cancellation");
            self.closing = Some(Box::pin(async move {
                if let Err(error) = session.close().await {
                    tracing::warn!(
                        event = "player_native_close_failed",
                        ?error,
                        "Native cleanup failed"
                    );
                }
            }));
        }
    }
    fn recover(&mut self) -> Update {
        self.sources.failed();
        self.retire();
        tracing::debug!(
            event = "player_source_failed",
            selection = self.target.as_ref().unwrap().key.selection,
            "Trying another authorized source"
        );
        self.status(PlayerPhase::Resolving, Some(PlayerError::Playback))
    }
    fn exhausted(&mut self, error: PlayerError) -> Update {
        self.failed = true;
        let mut update = self.status(PlayerPhase::Failed, Some(error));
        update.exhausted = matches!(error, PlayerError::Playback | PlayerError::NotFound);
        update
    }
    pub async fn close(&mut self) {
        self.target = None;
        self.sources.reset(None);
        self.retire();
        if let Some(mut closing) = self.closing.take() {
            closing.as_mut().await;
        }
        self.sources.close().await;
        tracing::info!(event = "player_native_closed", "Native effect owner joined");
    }
}
