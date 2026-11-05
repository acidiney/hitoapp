use super::{
    Key, Target, Update,
    controls::Controls,
    lookahead::{Event, Lookahead, WINDOW},
    sources::Sources,
};
use crate::{PlayerError, PlayerPhase, Session, Status};
use hito_core::{Catalog, PlaybackAccess};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
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
    lookahead: Lookahead,
    track: u64,
    crossed: Option<Key>,
    tail: Option<(Duration, Option<Duration>)>,
}
impl Native {
    pub fn new(catalog: Catalog, playback: PlaybackAccess) -> Self {
        Self {
            lookahead: Lookahead::new(catalog.clone(), playback.clone()),
            track: 0,
            crossed: None,
            tail: None,
            sources: Sources::new(catalog, playback),
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
        let key = target.as_ref().map(|target| target.key);
        let crossed = self.crossed.take();
        if self.target.as_ref().map(|target| target.key) != key {
            if self.session.is_some() && key.is_some() && crossed == key {
                self.adopt();
            } else {
                self.retire();
                self.sources.reset(key);
                self.failed = false;
                self.controls = Controls::default();
            }
        } else if crossed.is_some() {
            self.retire();
            self.sources.reset(key);
            self.failed = false;
            self.controls = Controls::default();
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
        self.sync_lookahead();
    }
    fn sync_lookahead(&mut self) {
        let (Some(target), Some(session)) = (&self.target, &self.session) else {
            return;
        };
        let wanted = target.next.filter(|_| {
            let snapshot = session.snapshot();
            let remaining = snapshot
                .format
                .and_then(|format| format.duration)
                .map(|duration| duration.saturating_sub(snapshot.position));
            let window = if self.lookahead.started() {
                WINDOW * 2
            } else {
                WINDOW
            };
            target.output
                && target.permitted
                && !self.failed
                && self.closing.is_none()
                && self.crossed.is_none()
                && snapshot.status == Status::Ready
                && snapshot.track == self.track
                && !snapshot.ended
                && self.controls.configured()
                && remaining.is_some_and(|remaining| remaining <= window)
        });
        let session = session.clone();
        let actor = target.actor.clone();
        self.lookahead.sync(wanted, &session, &actor);
    }
    fn adopt(&mut self) {
        std::mem::swap(&mut self.sources, &mut self.lookahead.sources);
        self.lookahead.reset();
        self.controls.adopt();
        self.failed = false;
        tracing::debug!(
            event = "player_lookahead_adopted",
            "The running stream continued into the prepared entry"
        );
    }
    fn crossing(&mut self) -> Option<Update> {
        let snapshot = self.session.as_ref()?.snapshot();
        if snapshot.track <= self.track {
            if snapshot.track == self.track {
                self.tail = Some((
                    snapshot.position,
                    snapshot.format.and_then(|format| format.duration),
                ));
            }
            return None;
        }
        self.track = snapshot.track;
        self.crossed = self.lookahead.key().filter(|_| self.lookahead.prepared());
        let (position, duration) = self.tail.take()?;
        let mut update = self.snapshot();
        update.ended = true;
        update.playing = true;
        update.position = duration.unwrap_or(position);
        update.duration_ms =
            duration.map(|duration| duration.as_millis().min(u64::MAX as u128) as u64);
        tracing::debug!(
            event = "player_boundary_crossed",
            adopted = self.crossed.is_some(),
            "Playback crossed into the next entry"
        );
        Some(update)
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
                        return if self.failed { self.status(PlayerPhase::Failed, Some(self.sources.failure())) } else { self.status(PlayerPhase::Resolving, None) };
                    }
                },
                grant = self.sources.next() => {
                    match grant {
                        Ok(Some(_)) => return self.status(PlayerPhase::Resolving, None),
                        Ok(None) => return self.exhausted(self.sources.failure()),
                        Err(error) => return self.exhausted(error),
                    }
                },
                result = self.controls.next() => {
                    if let Err(error) = result { return self.recover(Some(error)); }
                    if let Some(update) = self.crossing() { return update; }
                    return self.snapshot();
                },
                event = self.lookahead.next(self.session.as_ref()), if self.lookahead.active() => {
                    if event == Event::Broken { return self.recover(None); }
                },
                changed = async { self.observed.as_mut().unwrap().changed().await }, if self.observed.is_some() => {
                    if changed.is_err() { return self.recover(None); }
                    if let Some(update) = self.crossing() { return update; }
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
        self.lookahead.reset();
        self.track = 0;
        self.crossed = None;
        self.tail = None;
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
        let failure = failure_of(error.or_else(|| {
            self.session
                .as_ref()
                .and_then(|session| session.snapshot().error)
        }));
        self.sources.failed(failure);
        self.retire();
        tracing::debug!(
            event = "player_source_failed",
            selection = self.target.as_ref().unwrap().key.selection,
            unverified,
            reason = ?failure,
            "Trying another authorized source"
        );
        self.status(
            PlayerPhase::Resolving,
            Some(if unverified {
                PlayerError::Unavailable
            } else {
                failure
            }),
        )
    }
    fn exhausted(&mut self, error: PlayerError) -> Update {
        self.failed = true;
        let mut update = self.status(PlayerPhase::Failed, Some(error));
        update.exhausted = matches!(
            error,
            PlayerError::Playback | PlayerError::OutputFormat | PlayerError::NotFound
        );
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

fn failure_of(error: Option<crate::Error>) -> PlayerError {
    match error {
        Some(crate::Error::Native(hito_audio::Error::OutputFormat)) => PlayerError::OutputFormat,
        _ => PlayerError::Playback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_output_format_failure_has_its_own_player_error() {
        assert_eq!(
            failure_of(Some(crate::Error::Native(hito_audio::Error::OutputFormat))),
            PlayerError::OutputFormat
        );
        for other in [
            Some(crate::Error::Native(hito_audio::Error::Decode)),
            Some(crate::Error::Native(hito_audio::Error::OutputUnavailable)),
            Some(crate::Error::Input),
            None,
        ] {
            assert_eq!(failure_of(other), PlayerError::Playback);
        }
    }
}
