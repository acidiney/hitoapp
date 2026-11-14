mod acquisition;

use super::{
    Key, Target, Update,
    controls::Controls,
    deferred::{BOUNDARY_WAIT, Outcome},
    lookahead::{Event, Lookahead, PREPARATION_WINDOW, WINDOW},
    sources::Sources,
};
use crate::{PlayerError, PlayerPhase, Session, Status};
use acquisition::{
    Acquisition, FinalizationId, FinalizationTaskResult, FinalizationTasks, Finalized,
};
use hito_core::{Catalog, PlaybackAccess};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio::time::{Instant, sleep_until};

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[cfg(test)]
struct FinalizationArm {
    entered: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
    handled: tokio::sync::oneshot::Sender<()>,
}

enum Boundary {
    Open,
    Holding(Pin<Box<tokio::time::Sleep>>, tokio::time::Instant),
    Released,
}

pub(crate) struct Native {
    catalog: Catalog,
    playback: PlaybackAccess,
    sources: Sources,
    acquisition: Option<Acquisition>,
    acquired: Option<(Key, uuid::Uuid)>,
    finalizing: FinalizationTasks,
    deadline_pending: Option<FinalizationId>,
    #[cfg(test)]
    finalization_handled: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(test)]
    finalization_arm: Option<FinalizationArm>,
    target: Option<Target>,
    session: Option<Arc<Session>>,
    observed: Option<watch::Receiver<crate::Snapshot>>,
    controls: Controls,
    closing: Option<Pending<()>>,
    failed: bool,
    suspended: bool,
    update: Option<Update>,
    lookahead: Lookahead,
    track: u64,
    crossed: Option<Key>,
    tail: Option<(Duration, Option<Duration>)>,
    boundary: Boundary,
}
impl Native {
    pub fn new(catalog: Catalog, playback: PlaybackAccess) -> Self {
        Self {
            catalog: catalog.clone(),
            playback: playback.clone(),
            lookahead: Lookahead::new(catalog.clone(), playback.clone()),
            track: 0,
            crossed: None,
            tail: None,
            boundary: Boundary::Open,
            sources: Sources::new(catalog, playback),
            acquisition: None,
            acquired: None,
            finalizing: FinalizationTasks::new(),
            deadline_pending: None,
            #[cfg(test)]
            finalization_handled: None,
            #[cfg(test)]
            finalization_arm: None,
            target: None,
            session: None,
            observed: None,
            controls: Controls::default(),
            closing: None,
            failed: false,
            suspended: false,
            update: None,
        }
    }
    fn start_acquisition(
        &mut self,
        key: Key,
        demand: uuid::Uuid,
        play_revision: u64,
        seek_id: Option<uuid::Uuid>,
        restoration_id: Option<uuid::Uuid>,
    ) {
        if self
            .acquisition
            .as_ref()
            .is_some_and(|run| run.key == key && run.demand == demand)
            || self.acquired == Some((key, demand))
        {
            return;
        }
        self.deadline_pending = None;
        self.acquisition = Some(Acquisition::start(
            key,
            demand,
            play_revision,
            seek_id,
            restoration_id,
        ));
    }
    fn spawn_finalization<D, A>(&mut self, id: FinalizationId, drain: D, access: A)
    where
        D: Future<Output = bool> + Send + 'static,
        A: Future<Output = Result<(), PlayerError>> + Send + 'static,
    {
        let pending: Pending<Finalized> = Box::pin(async move {
            let (drained, access) = tokio::join!(drain, access);
            Finalized {
                id,
                drained,
                access,
            }
        });
        self.finalizing.spawn(id, pending);
    }
    #[cfg(test)]
    pub(super) fn arm_finalizer_for_test(
        &mut self,
        entered: tokio::sync::oneshot::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
        handled: tokio::sync::oneshot::Sender<()>,
    ) {
        self.finalization_arm = Some(FinalizationArm {
            entered,
            release,
            handled,
        });
    }
    fn acquisition_progressed(&mut self) -> bool {
        let (Some(run), Some(target), Some(session)) = (
            self.acquisition.as_mut(),
            self.target.as_ref(),
            self.session.as_ref(),
        ) else {
            return false;
        };
        if run.key != target.key
            || !target.playing
            || !target.permitted
            || !target.output
            || run.play_revision != target.play_revision
            || run.seek_id != target.seek.map(|seek| seek.id)
            || run.restoration_id != target.restoration.map(|restoration| restoration.id)
            || !self.controls.configured()
        {
            return false;
        }
        let restored = self
            .controls
            .restored()
            .map(|restored| (restored.id, restored.baseline));
        run.observe(
            target,
            session.snapshot(),
            self.controls.configured(),
            self.controls.applied_seek(),
            restored,
        )
    }
    fn begin_deadline_finalization(&mut self) {
        let Some(run) = self.acquisition.take() else {
            return;
        };
        if !self.target.as_ref().is_some_and(|target| {
            target.key == run.key && target.playing && target.permitted && target.output
        }) {
            return;
        }
        let id = run.finalization_id();
        let elapsed_ms = run.started.elapsed().as_millis() as u64;
        tracing::info!(
            event = "player_acquisition_deadline",
            request_id = %run.demand,
            elapsed_ms,
            attempts = run.attempts,
            "Native playback acquisition deadline reached"
        );
        self.deadline_pending = Some(id);
        if let Some(session) = &self.session {
            session.stop();
        }
        let catalog = self.catalog.clone();
        let playback = self.playback.clone();
        let mut lookahead = std::mem::replace(
            &mut self.lookahead,
            Lookahead::new(catalog.clone(), playback.clone()),
        );
        self.observed = None;
        self.controls
            .retire(self.session.as_ref().map(|session| session.snapshot()));
        self.track = 0;
        self.crossed = None;
        self.tail = None;
        self.boundary = Boundary::Open;
        let session = self.session.take();
        let closing = self.closing.take();
        self.sources.reset(None);
        let mut fresh_sources = Sources::new(catalog.clone(), playback);
        fresh_sources.reset(Some(run.key));
        let mut sources = std::mem::replace(&mut self.sources, fresh_sources);
        self.update = Some(self.status(PlayerPhase::Resolving, None));
        let target = self.target.as_ref();
        let actor = target.map(|target| target.actor.clone());
        let recording = target.map(|target| target.recording);
        let drain = async move {
            let mut drained = true;
            lookahead.close().await;
            if let Some(session) = session {
                match Arc::into_inner(session) {
                    Some(session) => drained = session.close().await.is_ok(),
                    None => drained = false,
                }
            }
            if let Some(mut closing) = closing {
                closing.as_mut().await;
            }
            sources.close().await;
            drained
        };
        let access = async move {
            match (actor, recording) {
                (Some(actor), Some(recording)) => catalog
                    .recordings_by_ids(&actor, &[recording])
                    .await
                    .map(|_| ())
                    .map_err(PlayerError::from),
                _ => Err(PlayerError::Unavailable),
            }
        };
        self.spawn_finalization(id, drain, access);
    }
    fn finish_finalization(&mut self, result: Finalized) -> Option<Update> {
        if self.deadline_pending != Some(result.id) {
            return None;
        }
        self.deadline_pending = None;
        let target = self.target.as_ref()?;
        if target.key != result.id.key
            || !target.playing
            || !target.permitted
            || !target.output
            || target.play_revision != result.id.play_revision
            || target.seek.map(|seek| seek.id) != result.id.seek_id
            || target.restoration.map(|restoration| restoration.id) != result.id.restoration_id
        {
            return None;
        }
        if !result.drained {
            self.failed = true;
            return Some(self.status(PlayerPhase::Failed, Some(PlayerError::Playback)));
        }
        match result.access {
            Ok(()) => Some(self.exhausted(PlayerError::Playback)),
            Err(error) => {
                self.failed = true;
                Some(self.status(PlayerPhase::Failed, Some(error)))
            }
        }
    }
    fn finish_finalization_panic(&mut self, id: Option<FinalizationId>) -> Option<Update> {
        if !acquisition::failed_finalization_is_current(
            self.deadline_pending,
            id,
            self.target.as_ref().map(|target| target.key),
        ) {
            return None;
        }
        self.deadline_pending = None;
        self.failed = true;
        Some(self.status(PlayerPhase::Failed, Some(PlayerError::Playback)))
    }
    pub fn sync(&mut self, target: Option<Target>) {
        let key = target.as_ref().map(|target| target.key);
        let crossed = self.crossed.take();
        let mut suspended_now = false;
        if self.target.as_ref().map(|target| target.key) != key {
            self.acquisition = None;
            self.acquired = None;
            self.deadline_pending = None;
            self.update = None;
            self.suspended = false;
            if self.session.is_some() && key.is_some() && crossed == key {
                self.adopt();
            } else {
                self.retire();
                self.sources.reset(key);
                self.failed = false;
                self.controls = Controls::default();
            }
        } else if crossed.is_some() {
            self.acquisition = None;
            self.acquired = None;
            self.deadline_pending = None;
            self.update = None;
            self.retire();
            self.sources.reset(key);
            self.failed = false;
            self.controls = Controls::default();
        }
        let restoring_suspended_idle = self.suspended
            && target
                .as_ref()
                .is_some_and(|target| target.permitted && target.output && !target.playing)
            && self
                .target
                .as_ref()
                .is_some_and(|target| !target.permitted || !target.output);
        if let Some(restoration) = target.as_ref().and_then(|target| target.restoration)
            && self
                .target
                .as_ref()
                .and_then(|target| target.restoration)
                .map(|r| r.id)
                != Some(restoration.id)
        {
            self.update = None;
            if let Some(run) = &mut self.acquisition {
                run.restoration_id = Some(restoration.id);
                run.reset_baseline();
            }
            self.deadline_pending = None;
            self.controls.request_restoration(restoration);
        }
        if target.as_ref().is_some_and(|target| !target.permitted) {
            self.acquisition = None;
            self.acquired = None;
            self.deadline_pending = None;
            self.update = None;
            if self.target.as_ref().is_none_or(|target| target.permitted) {
                self.retire();
                self.sources.reset(target.as_ref().map(|target| target.key));
                self.failed = false;
            }
            self.target = target;
            return;
        }
        if target.as_ref().is_some_and(|target| !target.output) {
            self.acquisition = None;
            self.acquired = None;
            self.deadline_pending = None;
            self.update = None;
            if self.session.is_some() || self.sources.has_demand() {
                self.retire();
                self.sources.reset(target.as_ref().map(|target| target.key));
            }
            self.target = target;
            return;
        }
        if let Some(seek) = target.as_ref().and_then(|target| target.seek)
            && self.target.as_ref().and_then(|target| target.seek) != Some(seek)
        {
            self.update = None;
            if let Some(run) = &mut self.acquisition {
                run.seek_id = Some(seek.id);
                run.restoration_id = target
                    .as_ref()
                    .and_then(|target| target.restoration.map(|r| r.id));
                run.reset_baseline();
            }
            self.deadline_pending = None;
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
        } else if let Some(next) = target
            .as_ref()
            .filter(|next| !next.playing && self.target.as_ref().is_some_and(|old| old.playing))
        {
            self.acquisition = None;
            self.deadline_pending = None;
            self.update = None;
            if self.sources.has_demand() && (self.session.is_none() || !self.controls.configured())
            {
                self.suspended = true;
                suspended_now = true;
                self.update = None;
                self.sources.reset(Some(next.key));
                self.retire();
                self.failed = false;
                tracing::debug!(
                    event = "player_acquisition_suspended",
                    selection = next.key.selection,
                    outcome = "paused",
                    "Suspending unconfigured native acquisition"
                );
            } else if self.controls.busy() {
                self.retire();
                self.failed = false;
            }
        } else if target.as_ref().is_some_and(|next| {
            next.playing
                && self
                    .target
                    .as_ref()
                    .is_some_and(|old| old.play_revision != next.play_revision)
        }) {
            self.acquisition = None;
            self.deadline_pending = None;
            self.update = None;
            if self.failed {
                self.sources.reset(target.as_ref().map(|target| target.key));
            }
            self.failed = false;
        }
        if self.suspended && target.as_ref().is_some_and(|target| target.playing) {
            self.suspended = false;
            tracing::debug!(
                event = "player_acquisition_resumed",
                selection = target.as_ref().unwrap().key.selection,
                outcome = "fresh_demand",
                "Resuming native acquisition"
            );
        }
        self.target = target;
        #[cfg(test)]
        if let Some(target) = self
            .target
            .as_ref()
            .filter(|target| target.playing && target.permitted && target.output)
            && let Some(demand) = self
                .sources
                .prepare_request(&target.actor, &target.recording.to_string())
            && let Some(arm) = self.finalization_arm.take()
        {
            let id = FinalizationId {
                key: target.key,
                demand,
                play_revision: target.play_revision,
                seek_id: target.seek.map(|seek| seek.id),
                restoration_id: target.restoration.map(|restoration| restoration.id),
            };
            self.deadline_pending = Some(id);
            self.finalization_handled = Some(arm.handled);
            self.spawn_finalization(id, async { true }, async move {
                let _ = arm.entered.send(());
                let _ = arm.release.await;
                Ok(())
            });
        }
        if suspended_now || restoring_suspended_idle {
            self.update = Some(self.status(PlayerPhase::Idle, None));
        }
        if self.suspended {
            return;
        }
        let acquisition_start = self.target.as_ref().and_then(|target| {
            if !target.playing || self.acquisition.is_some() || self.deadline_pending.is_some() {
                return None;
            }
            self.sources
                .prepare_request(&target.actor, &target.recording.to_string())
                .map(|demand| {
                    (
                        target.key,
                        demand,
                        target.play_revision,
                        target.seek.map(|seek| seek.id),
                        target.restoration.map(|restoration| restoration.id),
                    )
                })
        });
        if let Some((key, demand, play_revision, seek_id, restoration_id)) = acquisition_start {
            self.start_acquisition(key, demand, play_revision, seek_id, restoration_id);
        }
        let Some(target) = self.target.as_ref() else {
            return;
        };
        if self.closing.is_none()
            && self.session.is_none()
            && !self.failed
            && self.deadline_pending.is_none()
        {
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
                .prepare_request(&target.actor, &target.recording.to_string())
                .is_some()
                && self
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
        let snapshot = session.snapshot();
        let remaining = snapshot
            .format
            .and_then(|format| format.duration)
            .map(|duration| duration.saturating_sub(snapshot.position));
        let following = target.next.filter(|_| {
            target.output
                && target.permitted
                && !self.failed
                && self.closing.is_none()
                && self.crossed.is_none()
                && snapshot.status == Status::Ready
                && snapshot.track == self.track
                && (snapshot.position > Duration::ZERO
                    || (target.playing && !self.controls.busy() && snapshot.buffer_ready))
                && self.controls.configured()
        });
        let early = following.filter(|_| {
            let window = if self.lookahead.deferring() {
                PREPARATION_WINDOW * 2
            } else {
                PREPARATION_WINDOW
            };
            remaining.is_some_and(|remaining| remaining <= window)
        });
        let wanted = early.filter(|_| {
            let window = if self.lookahead.started() {
                WINDOW * 2
            } else {
                WINDOW
            };
            !snapshot.ended && remaining.is_some_and(|remaining| remaining <= window)
        });
        let session = session.clone();
        let actor = target.actor.clone();
        self.lookahead.sync(early, wanted, &session, &actor);
    }
    fn adopt(&mut self) {
        std::mem::swap(&mut self.sources, &mut self.lookahead.sources);
        self.lookahead.reset();
        self.controls.adopt();
        self.boundary = Boundary::Open;
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
        if self
            .acquisition
            .as_ref()
            .is_some_and(|run| Instant::now() >= run.deadline_at)
        {
            self.begin_deadline_finalization();
        }
        if let Some(update) = self.update.take() {
            return update;
        }
        loop {
            let acquisition = self
                .acquisition
                .as_ref()
                .map(|run| (run.attempts, run.started));
            tokio::select! {
                biased;
                _ = async {
                    if let Some(deadline) = self.acquisition.as_ref().map(|run| run.deadline_at) {
                        sleep_until(deadline).await;
                    } else {
                        std::future::pending().await
                    }
                }, if self.acquisition.is_some() => {
                    self.begin_deadline_finalization();
                    if let Some(update) = self.update.take() {
                        return update;
                    }
                },
                result = self.finalizing.join_next(), if !self.finalizing.is_empty() => {
                    match result {
                        Some(FinalizationTaskResult::Finished(result)) => {
                            if let Some(update) = self.finish_finalization(result) {
                                return update;
                            }
                            #[cfg(test)]
                            if let Some(handled) = self.finalization_handled.take() {
                                let _ = handled.send(());
                            }
                        }
                        Some(FinalizationTaskResult::Panicked(id)) => {
                            if let Some(update) = self.finish_finalization_panic(id) {
                                return update;
                                }
                            #[cfg(test)]
                            if let Some(handled) = self.finalization_handled.take() {
                                let _ = handled.send(());
                            }
                        }
                        None => (),
                    }
                },
                _ = async {
                    if let Some(backoff) = self.acquisition.as_ref().and_then(|run| run.backoff_at) {
                        sleep_until(backoff).await;
                    } else {
                        std::future::pending().await
                    }
                }, if self.acquisition.as_ref().is_some_and(|run| run.backoff_at.is_some()) => {
                    if let Some(run) = &mut self.acquisition {
                        run.backoff_at = None;
                        run.attempts = run.attempts.saturating_add(1);
                    }
                    if self.sources.restart_attempt() {
                        return self.status(PlayerPhase::Resolving, None);
                    }
                },
                _ = async { self.closing.as_mut().unwrap().await }, if self.closing.is_some() => {
                    self.closing = None;
                    if self.target.is_some() && !self.suspended {
                        return if self.failed { self.status(PlayerPhase::Failed, Some(self.sources.failure())) } else { self.status(PlayerPhase::Resolving, None) };
                    }
                },
                grant = self.sources.next_with_attempt(acquisition) => {
                    if self.suspended { continue; }
                    match grant {
                        Ok(Some(_)) => return self.status(PlayerPhase::Resolving, None),
                        Ok(None) => return self.exhausted(self.sources.failure()),
                        Err(failure) if failure.cancelled => {
                            self.sources.restart_attempt();
                            return self.status(PlayerPhase::Resolving, None);
                        },
                        Err(failure) if failure.retryable
                            && self.acquisition.as_ref().is_some_and(|run| run.attempts < 3) => {
                                let run = self.acquisition.as_mut().unwrap();
                                run.retrying = Some(crate::PlayerRetry {
                                    attempt: run.attempts + 1,
                                    of: 3,
                                });
                                let delay = match run.attempts {
                                    1 => Duration::from_millis(500),
                                    _ => Duration::from_millis(1000),
                                };
                                run.backoff_at = Some(Instant::now() + delay);
                                return self.status(PlayerPhase::Resolving, None);
                        },
                        Err(failure) => return self.exhausted(failure.error),
                    }
                },
                result = self.controls.next() => {
                    if let Err(error) = result { return self.recover(Some(error)); }
                    if let Some(update) = self.crossing() { return update; }
                    if self.acquisition_progressed()
                        && let Some(run) = self.acquisition.take()
                    {
                        self.acquired = Some((run.key, run.demand));
                    }
                    return self.held(self.snapshot());
                },
                event = self.lookahead.next(self.session.as_ref()), if self.lookahead.active() => {
                    if event == Event::Broken { return self.recover(None); }
                    if event == Event::Settled && matches!(self.boundary, Boundary::Holding(..)) {
                        return self.release("settled");
                    }
                },
                _ = async {
                    if let Boundary::Holding(timer, _) = &mut self.boundary { timer.as_mut().await }
                }, if matches!(self.boundary, Boundary::Holding(..)) => {
                    return self.release("timeout");
                },
                changed = async { self.observed.as_mut().unwrap().changed().await }, if self.observed.is_some() => {
                    if changed.is_err() { return self.recover(None); }
                    if let Some(update) = self.crossing() { return update; }
                    if self.acquisition_progressed()
                        && let Some(run) = self.acquisition.take()
                    {
                        self.acquired = Some((run.key, run.demand));
                    }
                    let update = self.snapshot();
                    if update.phase == PlayerPhase::Failed { return self.recover(None); }
                    return self.held(update);
                },
                else => std::future::pending().await,
            }
        }
    }
    fn held(&mut self, mut update: Update) -> Update {
        let Some(next) = self.target.as_ref().and_then(|target| target.next) else {
            return update;
        };
        if !update.ended || update.phase != PlayerPhase::Ready {
            return update;
        }
        if matches!(self.boundary, Boundary::Open) {
            if !self.lookahead.waiting_for(next.key) {
                return update;
            }
            self.boundary = Boundary::Holding(
                Box::pin(tokio::time::sleep(BOUNDARY_WAIT)),
                tokio::time::Instant::now(),
            );
            tracing::info!(
                event = "player_boundary_wait_started",
                "Holding the boundary while the next entry source is prepared"
            );
        }
        if matches!(self.boundary, Boundary::Holding(..)) {
            if !self.lookahead.waiting_for(next.key) {
                return self.release("settled");
            }
            update.ended = false;
            update.playing = false;
            update.phase = PlayerPhase::Preparing;
        }
        update
    }
    fn release(&mut self, reason: &'static str) -> Update {
        if let Boundary::Holding(_, since) =
            std::mem::replace(&mut self.boundary, Boundary::Released)
        {
            tracing::info!(
                event = "player_boundary_wait_finished",
                reason,
                ready = self.lookahead.outcome() == Some(Outcome::Ready),
                waited_ms = since.elapsed().as_millis() as u64,
                "Boundary hold ended"
            );
        }
        self.snapshot()
    }
    fn snapshot(&self) -> Update {
        let mut update = self.controls.snapshot(
            self.target.as_ref().unwrap(),
            self.sources
                .current()
                .map(|grant| grant.source_id().to_owned()),
            self.session.as_ref().unwrap().snapshot(),
        );
        update.retrying = self.retrying();
        update
    }
    fn status(&self, phase: PlayerPhase, error: Option<PlayerError>) -> Update {
        let mut update = self.controls.status(
            self.target.as_ref().unwrap(),
            self.sources
                .current()
                .map(|grant| grant.source_id().to_owned()),
            phase,
            error,
        );
        update.retrying = self.retrying();
        update
    }
    fn retrying(&self) -> Option<crate::PlayerRetry> {
        self.acquisition.as_ref().and_then(|run| run.retrying)
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
        self.boundary = Boundary::Open;
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
        self.acquired = None;
        let unverified = error == Some(crate::Error::Access)
            || self.session.as_ref().is_some_and(|session| {
                let snapshot = session.snapshot();
                snapshot.status == Status::Revoked || snapshot.error == Some(crate::Error::Access)
            });
        if unverified || error == Some(crate::Error::Native(hito_audio::Error::OutputUnavailable)) {
            self.acquisition = None;
            self.deadline_pending = None;
        }
        let failure = failure_of(error.or_else(|| {
            self.session
                .as_ref()
                .and_then(|session| session.snapshot().error)
        }));
        let retryable = matches!(
            error.or_else(|| {
                self.session
                    .as_ref()
                    .and_then(|session| session.snapshot().error)
            }),
            Some(crate::Error::Provider(
                hito_core::PlaybackFailureReason::ProviderUnavailable
            ))
        );
        self.sources.failed(failure, retryable);
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
        self.acquisition = None;
        self.acquired = None;
        self.deadline_pending = None;
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
        self.acquisition = None;
        self.acquired = None;
        self.deadline_pending = None;
        self.sources.reset(None);
        self.retire();
        if let Some(mut closing) = self.closing.take() {
            closing.as_mut().await;
        }
        self.lookahead.close().await;
        self.sources.close().await;
        while self.finalizing.join_next().await.is_some() {}
        tracing::info!(event = "player_native_closed", "Native effect owner joined");
    }
}

fn failure_of(error: Option<crate::Error>) -> PlayerError {
    match error {
        Some(crate::Error::Native(hito_audio::Error::OutputFormat)) => PlayerError::OutputFormat,
        Some(crate::Error::Provider(
            hito_core::PlaybackFailureReason::CredentialsRequired
            | hito_core::PlaybackFailureReason::ProviderUnavailable
            | hito_core::PlaybackFailureReason::ProviderRefused
            | hito_core::PlaybackFailureReason::InvalidProviderResponse,
        )) => PlayerError::Playback,
        _ => PlayerError::Playback,
    }
}

#[cfg(test)]
mod tests;
