use super::{Target, Update, controls::Controls, sources::Sources};
use crate::{PlayerError, PlayerPhase, Session, Status};
use hito_core::Catalog;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::watch;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub(crate) struct Native {
    sources: Sources,
    target: Option<Target>,
    session: Option<Arc<Session>>,
    observed: Option<watch::Receiver<crate::Snapshot>>,
    controls: Controls,
    closing: Option<Pending<()>>,
    failed: bool,
    update: Option<Update>,
}
impl Native {
    pub fn new(catalog: Catalog) -> Self {
        Self {
            sources: Sources::new(catalog),
            target: None,
            session: None,
            observed: None,
            controls: Controls::default(),
            closing: None,
            failed: false,
            update: None,
        }
    }
    pub fn sync(&mut self, target: Option<Target>) {
        if self.target.as_ref().map(|target| target.key) != target.as_ref().map(|target| target.key)
        {
            self.retire();
            self.sources.reset(target.as_ref().map(|target| target.key));
            self.failed = false;
            self.controls = Controls::default();
        }
        if target.as_ref().is_some_and(|target| !target.permitted) {
            if self.target.as_ref().is_none_or(|target| target.permitted) {
                self.retire();
                self.sources.reset(target.as_ref().map(|target| target.key));
                self.failed = false;
            }
            self.target = target;
            return;
        }
        if target.as_ref().is_some_and(|target| !target.output) {
            if self.session.is_some() {
                self.retire();
                self.sources.reset(target.as_ref().map(|target| target.key));
            }
            self.target = target;
            return;
        }
        if let Some(restoration) = target.as_ref().and_then(|target| target.restoration)
            && self
                .target
                .as_ref()
                .and_then(|target| target.restoration)
                .map(|r| r.id)
                != Some(restoration.id)
        {
            self.controls.request_restoration(restoration);
        }
        if let Some(seek) = target.as_ref().and_then(|target| target.seek)
            && self.target.as_ref().and_then(|target| target.seek) != Some(seek)
        {
            if self.controls.busy() {
                self.retire();
            }
            if self.failed {
                self.sources.reset(target.as_ref().map(|target| target.key));
                self.failed = false;
                tracing::debug!(event = "player_failed_seek_retry", command_id = %seek.id, "Retrying authorized input after an explicit seek");
            }
            self.controls.request_seek(seek.position_ms);
        } else if target.as_ref().is_some_and(|next| {
            self.target
                .as_ref()
                .is_some_and(|old| old.volume != next.volume || old.speed != next.speed)
        }) && self.controls.busy()
        {
            self.retire();
        } else if target.as_ref().is_some_and(|next| {
            !next.playing && self.target.as_ref().is_some_and(|old| old.playing)
        }) && self.controls.busy()
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
                self.controls.opened();
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
        if let Some(session) = &self.session
            && !self.failed
        {
            self.controls.sync(session, target);
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
                result = self.controls.next() => {
                    if let Err(error) = result { return self.recover(Some(error)); }
                    return self.snapshot();
                },
                changed = async { self.observed.as_mut().unwrap().changed().await }, if self.observed.is_some() => {
                    if changed.is_err() { return self.recover(None); }
                    let update = self.snapshot();
                    if update.phase == PlayerPhase::Failed { return self.recover(None); }
                    return update;
                },
                else => std::future::pending().await,
            }
        }
    }
    fn snapshot(&self) -> Update {
        self.controls.snapshot(
            self.target.as_ref().unwrap(),
            self.sources
                .current()
                .map(|grant| grant.source_id().to_owned()),
            self.session.as_ref().unwrap().snapshot(),
        )
    }
    fn status(&self, phase: PlayerPhase, error: Option<PlayerError>) -> Update {
        self.controls.status(
            self.target.as_ref().unwrap(),
            self.sources
                .current()
                .map(|grant| grant.source_id().to_owned()),
            phase,
            error,
        )
    }
    fn retire(&mut self) {
        let snapshot = self.session.as_ref().map(|session| session.snapshot());
        if let Some(session) = &self.session {
            session.stop();
        }
        self.controls.retire(snapshot);
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
    fn recover(&mut self, error: Option<crate::Error>) -> Update {
        let unverified = error == Some(crate::Error::Access)
            || self.session.as_ref().is_some_and(|session| {
                let snapshot = session.snapshot();
                snapshot.status == Status::Revoked || snapshot.error == Some(crate::Error::Access)
            });
        self.sources.failed();
        self.retire();
        tracing::debug!(
            event = "player_source_failed",
            selection = self.target.as_ref().unwrap().key.selection,
            unverified,
            "Trying another authorized source"
        );
        self.status(
            PlayerPhase::Resolving,
            Some(if unverified {
                PlayerError::Unavailable
            } else {
                PlayerError::Playback
            }),
        )
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
