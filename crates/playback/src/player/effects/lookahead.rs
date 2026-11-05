use super::{
    Key, NextTarget,
    deferred::{Deferral, Outcome, PREPARATION_BOUND},
    sources::Sources,
};
use crate::Session;
use hito_core::{Actor, Catalog, PlaybackAccess};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use uuid::Uuid;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub(super) const WINDOW: Duration = Duration::from_secs(20);
pub(super) const PREPARATION_WINDOW: Duration = PREPARATION_BOUND;

enum Stage {
    Idle,
    Resolving,
    Awaiting,
    Offering(Pending<crate::Result<()>>),
    Offered,
    Failed,
}

#[derive(PartialEq, Eq)]
pub(super) enum Event {
    Progress,
    Settled,
    Broken,
}

pub(super) struct Lookahead {
    pub sources: Sources,
    catalog: Catalog,
    key: Option<Key>,
    stage: Stage,
    cancelling: Option<Pending<bool>>,
    demand: Option<(Actor, Uuid)>,
    early_key: Option<Key>,
    deferral: Option<Deferral>,
    outcome: Option<Outcome>,
    draining: Option<Pending<()>>,
}

impl Lookahead {
    pub fn new(catalog: Catalog, playback: PlaybackAccess) -> Self {
        Self {
            sources: Sources::cached(catalog.clone(), playback),
            catalog,
            key: None,
            stage: Stage::Idle,
            cancelling: None,
            demand: None,
            early_key: None,
            deferral: None,
            outcome: None,
            draining: None,
        }
    }

    pub fn started(&self) -> bool {
        self.key.is_some()
    }

    pub fn key(&self) -> Option<Key> {
        self.key
    }

    pub fn prepared(&self) -> bool {
        matches!(self.stage, Stage::Offering(_) | Stage::Offered)
    }

    pub fn deferring(&self) -> bool {
        self.early_key.is_some()
    }

    pub fn waiting_for(&self, key: Key) -> bool {
        self.early_key == Some(key) && self.deferral.is_some()
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn active(&self) -> bool {
        self.cancelling.is_some()
            || self.deferral.is_some()
            || self.draining.is_some()
            || matches!(self.stage, Stage::Resolving | Stage::Offering(_))
    }

    fn stop_deferral(&mut self) {
        if let Some(deferral) = self.deferral.take() {
            let cancelled = deferral.cancel();
            self.draining = Some(match self.draining.take() {
                Some(earlier) => Box::pin(async move {
                    earlier.await;
                    cancelled.await;
                }),
                None => cancelled,
            });
            tracing::debug!(
                event = "player_deferral_cancelled",
                "Next entry source preparation withdrawn"
            );
        }
    }

    pub fn sync(
        &mut self,
        early: Option<NextTarget>,
        wanted: Option<NextTarget>,
        session: &Arc<Session>,
        actor: &Actor,
    ) {
        let early_key = early.map(|next| next.key);
        if self.early_key != early_key {
            self.stop_deferral();
            self.outcome = None;
            self.early_key = early_key;
        }
        if let Some(next) = early
            && self.deferral.is_none()
            && self.outcome.is_none()
        {
            self.deferral = Some(Deferral::start(
                self.catalog.clone(),
                actor.clone(),
                next.recording,
            ));
            tracing::debug!(
                event = "player_deferral_started",
                selection = next.key.selection,
                "Preparing a source for the next entry if none is ready"
            );
        }
        self.demand = wanted.map(|next| (actor.clone(), next.recording));
        let wanted_key = wanted.map(|next| next.key);
        if self.key != wanted_key {
            if self.prepared() {
                let session = session.clone();
                self.cancelling = Some(Box::pin(async move {
                    for _ in 0..50 {
                        match session.cancel_next().await {
                            Ok(_) => return true,
                            Err(crate::Error::Busy) => {
                                tokio::time::sleep(Duration::from_millis(10)).await
                            }
                            Err(_) => return false,
                        }
                    }
                    false
                }));
                tracing::debug!(
                    event = "player_lookahead_cancelled",
                    "Prepared next entry withdrawn"
                );
            }
            self.sources.reset(wanted_key);
            self.key = wanted_key;
            self.stage = Stage::Idle;
        }
        if let Some(next) = wanted
            && matches!(self.stage, Stage::Idle)
            && self.cancelling.is_none()
        {
            self.sources.request(actor, &next.recording.to_string());
            self.stage = Stage::Resolving;
            tracing::debug!(
                event = "player_lookahead_started",
                selection = next.key.selection,
                "Resolving a source for the next entry"
            );
        }
    }

    pub fn reset(&mut self) {
        self.key = None;
        self.stage = Stage::Idle;
        self.cancelling = None;
        self.demand = None;
        self.stop_deferral();
        self.early_key = None;
        self.outcome = None;
        self.sources.reset(None);
    }

    pub async fn close(&mut self) {
        self.reset();
        if let Some(draining) = self.draining.take() {
            draining.await;
        }
    }

    pub async fn next(&mut self, session: Option<&Arc<Session>>) -> Event {
        loop {
            tokio::select! {
                biased;
                done = async { self.cancelling.as_mut().unwrap().await }, if self.cancelling.is_some() => {
                    self.cancelling = None;
                    return if done { Event::Progress } else { Event::Broken };
                },
                result = async {
                    match &mut self.stage {
                        Stage::Offering(pending) => pending.await,
                        _ => std::future::pending().await,
                    }
                }, if matches!(self.stage, Stage::Offering(_)) => {
                    self.stage = if result.is_ok() { Stage::Offered } else { Stage::Failed };
                    tracing::debug!(event = "player_lookahead_offered", accepted = result.is_ok(), "Next entry offered to the running stream");
                    return Event::Progress;
                },
                _ = async { self.draining.as_mut().unwrap().await }, if self.draining.is_some() => {
                    self.draining = None;
                    return Event::Progress;
                },
                outcome = async { self.deferral.as_mut().unwrap().finished().await }, if self.deferral.is_some() => {
                    self.deferral = None;
                    self.outcome = Some(outcome);
                    if outcome == Outcome::Ready
                        && matches!(self.stage, Stage::Awaiting)
                        && let Some((actor, recording)) = self.demand.clone()
                    {
                        self.sources.reset(self.key);
                        self.sources.request(&actor, &recording.to_string());
                        self.stage = Stage::Resolving;
                    } else if matches!(self.stage, Stage::Awaiting) {
                        self.stage = Stage::Failed;
                    }
                    return Event::Settled;
                },
                grant = self.sources.next(), if matches!(self.stage, Stage::Resolving) => {
                    match (grant, session) {
                        (Ok(Some(grant)), Some(session)) => {
                            let session = session.clone();
                            self.stage = Stage::Offering(Box::pin(async move { session.prepare_next(grant).await }));
                        }
                        _ if self.deferral.is_some() => self.stage = Stage::Awaiting,
                        _ => self.stage = Stage::Failed,
                    }
                    return Event::Progress;
                },
                else => std::future::pending().await,
            }
        }
    }
}
