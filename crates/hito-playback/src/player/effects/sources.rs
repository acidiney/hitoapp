use super::Key;
use crate::PlayerError;
use hito_core::{Actor, Catalog, PlaybackCandidates, PlaybackSource};
use std::{future::Future, pin::Pin};
use tokio::sync::oneshot;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type Read = Result<(PlaybackCandidates, Option<PlaybackSource>), PlayerError>;
#[derive(Clone)]
struct Demand {
    key: Key,
    actor: Actor,
    recording: String,
}
struct Refresh {
    cancel: Option<oneshot::Sender<()>>,
    done: Pending<(Key, Result<bool, PlayerError>)>,
}
pub(super) struct Sources {
    catalog: Catalog,
    key: Option<Key>,
    demand: Option<Demand>,
    candidates: Option<PlaybackCandidates>,
    current: Option<PlaybackSource>,
    reading: Option<Pending<Read>>,
    refreshing: Option<Refresh>,
    refreshed: bool,
    needs_refresh: bool,
    exhausted: bool,
    error: PlayerError,
}
impl Sources {
    pub fn new(catalog: Catalog) -> Self {
        Self {
            catalog,
            key: None,
            demand: None,
            candidates: None,
            current: None,
            reading: None,
            refreshing: None,
            refreshed: false,
            needs_refresh: false,
            exhausted: false,
            error: PlayerError::Playback,
        }
    }
    pub fn reset(&mut self, key: Option<Key>) {
        if let Some(refresh) = &mut self.refreshing
            && let Some(cancel) = refresh.cancel.take()
        {
            let _ = cancel.send(());
        }
        self.key = key;
        self.demand = None;
        self.candidates = None;
        self.current = None;
        self.reading = None;
        self.refreshed = false;
        self.needs_refresh = false;
        self.exhausted = false;
        self.error = PlayerError::Playback;
    }
    pub fn current(&self) -> Option<&PlaybackSource> {
        self.current.as_ref()
    }
    pub fn failed(&mut self) {
        self.current = None;
    }
    pub fn request(&mut self, actor: &Actor, recording: &str) -> bool {
        if self.demand.is_none() {
            let Some(key) = self.key else {
                return false;
            };
            self.demand = Some(Demand {
                key,
                actor: actor.clone(),
                recording: recording.to_owned(),
            });
        }
        self.start()
    }
    fn start(&mut self) -> bool {
        if self.current.is_some() || self.reading.is_some() || self.exhausted {
            return false;
        }
        let Some(demand) = self.demand.clone() else {
            return false;
        };
        if self.needs_refresh {
            if self.refreshing.is_some() {
                return false;
            }
            self.refreshed = true;
            self.needs_refresh = false;
            let catalog = self.catalog.clone();
            let (cancel, cancelled) = oneshot::channel();
            self.refreshing = Some(Refresh {
                cancel: Some(cancel),
                done: Box::pin(async move {
                    let result = catalog
                        .refresh_playback_sources_until(&demand.actor, &demand.recording, async {
                            let _ = cancelled.await;
                        })
                        .await
                        .map_err(PlayerError::from);
                    (demand.key, result)
                }),
            });
            return true;
        }
        if self.refreshing.as_ref().is_some_and(|_| self.refreshed) {
            return false;
        }
        let catalog = self.catalog.clone();
        let candidates = self.candidates.take();
        self.reading = Some(Box::pin(async move {
            let mut candidates = match candidates {
                Some(candidates) => candidates,
                None => catalog
                    .playback_candidates(&demand.actor, &demand.recording)
                    .await
                    .map_err(PlayerError::from)?,
            };
            let grant = candidates.next().await.map_err(PlayerError::from)?;
            Ok((candidates, grant))
        }));
        true
    }
    pub async fn next(&mut self) -> Result<Option<PlaybackSource>, PlayerError> {
        loop {
            tokio::select! {
                biased;
                (key, result) = async { self.refreshing.as_mut().unwrap().done.as_mut().await }, if self.refreshing.is_some() => {
                    self.refreshing = None;
                    if self.key == Some(key) {
                        self.candidates = None;
                        tracing::debug!(event = "player_sources_refreshed", success = result.is_ok(), selection = key.selection, "Provider refresh finished; reading current sources");
                        if result == Err(PlayerError::Unauthorized) { self.exhausted = true; return Err(PlayerError::Unauthorized); }
                    }
                    self.start();
                },
                result = async { self.reading.as_mut().unwrap().await }, if self.reading.is_some() => {
                    self.reading = None;
                    match result {
                        Ok((candidates, Some(grant))) => {
                            self.candidates = Some(candidates);
                            self.current = Some(grant.clone());
                            tracing::debug!(event = "player_source_selected", source_id = grant.source_id(), refreshed = self.refreshed, "Authorized playback source selected");
                            return Ok(Some(grant));
                        },
                        Ok((candidates, None)) => self.candidates = Some(candidates),
                        Err(error) => {
                            self.error = error;
                            if matches!(error, PlayerError::Unauthorized | PlayerError::Unavailable) { self.exhausted = true; return Err(error); }
                        },
                    }
                    if self.refreshed {
                        self.exhausted = true;
                        tracing::debug!(event = "player_sources_exhausted", "Cached and refreshed sources exhausted");
                        return Err(self.error);
                    }
                    self.needs_refresh = true;
                    self.start();
                },
                else => std::future::pending().await,
            }
        }
    }
    pub async fn close(&mut self) {
        self.reset(None);
        if let Some(mut refresh) = self.refreshing.take() {
            let _ = refresh.done.as_mut().await;
        }
    }
}
