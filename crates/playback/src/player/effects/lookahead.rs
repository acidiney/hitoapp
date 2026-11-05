use super::{Key, NextTarget, sources::Sources};
use crate::Session;
use hito_core::{Actor, Catalog, PlaybackAccess};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub(super) const WINDOW: Duration = Duration::from_secs(20);

enum Stage {
    Idle,
    Resolving,
    Offering(Pending<crate::Result<()>>),
    Offered,
    Failed,
}

#[derive(PartialEq, Eq)]
pub(super) enum Event {
    Progress,
    Broken,
}

pub(super) struct Lookahead {
    pub sources: Sources,
    key: Option<Key>,
    stage: Stage,
    cancelling: Option<Pending<bool>>,
}

impl Lookahead {
    pub fn new(catalog: Catalog, playback: PlaybackAccess) -> Self {
        Self {
            sources: Sources::cached(catalog, playback),
            key: None,
            stage: Stage::Idle,
            cancelling: None,
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

    pub fn active(&self) -> bool {
        self.cancelling.is_some() || matches!(self.stage, Stage::Resolving | Stage::Offering(_))
    }

    pub fn sync(&mut self, wanted: Option<NextTarget>, session: &Arc<Session>, actor: &Actor) {
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
        self.sources.reset(None);
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
                grant = self.sources.next(), if matches!(self.stage, Stage::Resolving) => {
                    match (grant, session) {
                        (Ok(Some(grant)), Some(session)) => {
                            let session = session.clone();
                            self.stage = Stage::Offering(Box::pin(async move { session.prepare_next(grant).await }));
                        }
                        _ => self.stage = Stage::Failed,
                    }
                    return Event::Progress;
                },
                else => std::future::pending().await,
            }
        }
    }
}
