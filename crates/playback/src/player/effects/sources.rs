use super::Key;
use crate::PlayerError;
mod discovery;
use discovery::{Discovery, Outcome};
use hito_core::{
    Actor, CandidatesEnd, Catalog, PlaybackAccess, PlaybackCandidates, PlaybackSource,
};
use std::{future::Future, pin::Pin};
use tokio::sync::oneshot;
use uuid::Uuid;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type Read = Result<(PlaybackCandidates, Option<PlaybackSource>), PlayerError>;
#[derive(Clone)]
struct Demand {
    id: Uuid,
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
    playback: PlaybackAccess,
    key: Option<Key>,
    demand: Option<Demand>,
    candidates: Option<PlaybackCandidates>,
    current: Option<PlaybackSource>,
    reading: Option<Pending<Read>>,
    refreshing: Option<Refresh>,
    discovering: Option<Discovery>,
    discovery_allowed: bool,
    refreshed: bool,
    refresh_allowed: bool,
    needs_refresh: bool,
    exhausted: bool,
    error: PlayerError,
}
impl Sources {
    pub fn new(catalog: Catalog, playback: PlaybackAccess) -> Self {
        Self::with_refresh(catalog, playback, true)
    }
    pub fn cached(catalog: Catalog, playback: PlaybackAccess) -> Self {
        Self::with_refresh(catalog, playback, false)
    }
    fn with_refresh(catalog: Catalog, playback: PlaybackAccess, refresh_allowed: bool) -> Self {
        Self {
            catalog,
            playback,
            key: None,
            demand: None,
            candidates: None,
            current: None,
            reading: None,
            refreshing: None,
            discovering: None,
            discovery_allowed: refresh_allowed,
            refreshed: !refresh_allowed,
            refresh_allowed,
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
        if let Some(discovery) = &self.discovering {
            discovery.cancel();
        }
        self.discovery_allowed = self.refresh_allowed;
        self.key = key;
        self.demand = None;
        self.candidates = None;
        self.current = None;
        self.reading = None;
        self.refreshed = !self.refresh_allowed;
        self.needs_refresh = false;
        self.exhausted = false;
        self.error = PlayerError::Playback;
    }
    pub fn current(&self) -> Option<&PlaybackSource> {
        self.current.as_ref()
    }
    pub fn failed(&mut self, error: PlayerError) {
        self.current = None;
        self.error = error;
    }
    pub fn failure(&self) -> PlayerError {
        self.error
    }
    pub fn request(&mut self, actor: &Actor, recording: &str) -> bool {
        if self.demand.is_none() {
            let Some(key) = self.key else {
                return false;
            };
            self.demand = Some(Demand {
                id: Uuid::new_v4(),
                key,
                actor: actor.clone(),
                recording: recording.to_owned(),
            });
        }
        self.start()
    }
    fn start(&mut self) -> bool {
        if self.current.is_some()
            || self.reading.is_some()
            || self.discovering.is_some()
            || self.exhausted
        {
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
        let playback = self.playback.clone();
        let candidates = self.candidates.take();
        self.reading = Some(Box::pin(async move {
            let mut candidates = match candidates {
                Some(candidates) => candidates,
                None => playback
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
                report = async { self.discovering.as_mut().unwrap().finished().await }, if self.discovering.is_some() => {
                    self.discovering = None;
                    if self.key != Some(report.key)
                        || self.demand.as_ref().is_none_or(|demand| demand.id != report.demand)
                    {
                        self.start();
                        continue;
                    }
                    match report.outcome {
                        Outcome::Ready => {
                            self.candidates = None;
                            self.start();
                        },
                        Outcome::Failed(error) => {
                            self.error = merged(self.error, error);
                            self.exhausted = true;
                            return Err(self.error);
                        },
                        Outcome::Exhausted | Outcome::Cancelled | Outcome::Deadline => {
                            self.exhausted = true;
                            return Err(self.error);
                        },
                    }
                },
                result = async { self.reading.as_mut().unwrap().await }, if self.reading.is_some() => {
                    self.reading = None;
                    match result {
                        Ok((candidates, Some(grant))) => {
                            self.discovery_allowed = false;
                            self.candidates = Some(candidates);
                            self.current = Some(grant.clone());
                            tracing::debug!(event = "player_source_selected", source_id = grant.source_id(), refreshed = self.refreshed, "Authorized playback source selected");
                            return Ok(Some(grant));
                        },
                        Ok((candidates, None)) => self.candidates = Some(candidates),
                        Err(error) => {
                            self.error = merged(self.error, error);
                            if matches!(error, PlayerError::Unauthorized | PlayerError::Unavailable) { self.exhausted = true; return Err(self.error); }
                        },
                    }
                    if self.refreshed {
                        if self.discovery_allowed
                            && self.error != PlayerError::OutputFormat
                            && self.candidates.as_ref().and_then(PlaybackCandidates::ended)
                                == Some(CandidatesEnd::Exhausted)
                        {
                            self.discovery_allowed = false;
                            self.discovering = Some(Discovery::start(
                                self.catalog.clone(), self.demand.clone().unwrap(),
                            ));
                            continue;
                        }
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
        if let Some(mut discovery) = self.discovering.take() {
            let _ = discovery.finished().await;
        }
    }
}

fn merged(current: PlayerError, incoming: PlayerError) -> PlayerError {
    if current == PlayerError::OutputFormat && incoming != PlayerError::Unauthorized {
        current
    } else {
        incoming
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hito_core::{ChooseSource, test_support::provider_audio::AudioLibrary};
    use uuid::Uuid;

    fn key() -> Key {
        Key {
            scope: Uuid::new_v4(),
            selection: 1,
            entry: Uuid::new_v4(),
            start: None,
        }
    }

    #[tokio::test]
    async fn the_next_entry_source_honours_the_manual_choice_and_never_leaves_it() {
        let library = AudioLibrary::start().await;
        let alternate = library.save_copy_and_attach_alternate().await;
        library
            .hito
            .catalog
            .choose_source(
                &library.actor,
                &library.source.recording_id,
                ChooseSource {
                    request_id: Uuid::new_v4(),
                    source_id: alternate.second.parse().unwrap(),
                },
            )
            .await
            .unwrap();
        let mut sources =
            Sources::cached(library.hito.catalog.clone(), library.hito.playback.clone());
        sources.reset(Some(key()));
        assert!(sources.request(&library.actor, &library.source.recording_id));
        let grant = sources.next().await.unwrap().unwrap();
        assert_eq!(grant.source_id(), alternate.second);
        sources.failed(PlayerError::Playback);
        assert!(sources.request(&library.actor, &library.source.recording_id));
        assert!(sources.next().await.is_err());
        assert!(sources.current().is_none());
    }

    #[test]
    fn a_device_reason_survives_later_lookup_failures_but_not_lost_authorization() {
        for later in [PlayerError::NotFound, PlayerError::Unavailable] {
            assert_eq!(
                merged(PlayerError::OutputFormat, later),
                PlayerError::OutputFormat
            );
        }
        assert_eq!(
            merged(PlayerError::OutputFormat, PlayerError::Unauthorized),
            PlayerError::Unauthorized
        );
        assert_eq!(
            merged(PlayerError::Playback, PlayerError::NotFound),
            PlayerError::NotFound
        );
    }
}
