use super::Key;
use crate::PlayerError;
mod discovery;
use discovery::{Discovery, Outcome};
use hito_core::{
    Actor, CandidatesEnd, Catalog, PlaybackAccess, PlaybackCandidates, PlaybackSource,
};
use std::{future::Future, pin::Pin};
use tokio::sync::oneshot;
use tokio::time::Instant;
use uuid::Uuid;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type RefreshResult = (Key, Uuid, Result<bool, (PlayerError, bool)>);
type Read = Result<(PlaybackCandidates, Option<PlaybackSource>), PlayerError>;
#[derive(Debug)]
pub(super) struct PassFailure {
    pub error: PlayerError,
    pub retryable: bool,
    pub cancelled: bool,
}
#[derive(Clone)]
struct Demand {
    id: Uuid,
    key: Key,
    actor: Actor,
    recording: String,
}
struct Refresh {
    cancel: Option<oneshot::Sender<()>>,
    done: Pending<RefreshResult>,
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
    pass_retryable: bool,
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
            pass_retryable: true,
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
        self.pass_retryable = true;
    }
    pub fn current(&self) -> Option<&PlaybackSource> {
        self.current.as_ref()
    }
    pub fn has_demand(&self) -> bool {
        self.demand.is_some() && !self.exhausted
    }
    pub fn failed(&mut self, error: PlayerError, retryable: bool) {
        self.current = None;
        self.error = error;
        self.pass_retryable &= retryable;
    }
    pub fn failure(&self) -> PlayerError {
        self.error
    }
    pub fn prepare_request(&mut self, actor: &Actor, recording: &str) -> Option<Uuid> {
        if self.demand.is_none() {
            let key = self.key?;
            self.demand = Some(Demand {
                id: Uuid::new_v4(),
                key,
                actor: actor.clone(),
                recording: recording.to_owned(),
            });
        }
        self.demand.as_ref().map(|demand| demand.id)
    }
    pub fn request(&mut self, actor: &Actor, recording: &str) -> bool {
        self.prepare_request(actor, recording);
        self.start()
    }
    pub fn restart_attempt(&mut self) -> bool {
        if self.reading.is_some() || self.refreshing.is_some() || self.discovering.is_some() {
            return false;
        }
        self.candidates = None;
        self.current = None;
        self.exhausted = false;
        self.error = PlayerError::Playback;
        self.refreshed = true;
        self.needs_refresh = false;
        self.discovery_allowed = true;
        self.pass_retryable = true;
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
            let demand_id = demand.id;
            let (cancel, cancelled) = oneshot::channel();
            self.refreshing = Some(Refresh {
                cancel: Some(cancel),
                done: Box::pin(async move {
                    let result = catalog
                        .refresh_playback_sources_until(&demand.actor, &demand.recording, async {
                            let _ = cancelled.await;
                        })
                        .await
                        .map_err(|error| {
                            let retryable = matches!(
                                &error,
                                hito_core::Error::PlaybackProvider {
                                    reason: hito_core::PlaybackFailureReason::ProviderUnavailable,
                                    ..
                                }
                            );
                            let player_error = match error {
                                hito_core::Error::PlaybackProvider { .. } => PlayerError::Playback,
                                hito_core::Error::PluginCredentialsRequired(_)
                                | hito_core::Error::AuthorizationRequired(_) => {
                                    PlayerError::Playback
                                }
                                error => PlayerError::from(error),
                            };
                            (player_error, retryable)
                        });
                    (demand.key, demand_id, result)
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
    pub async fn next(&mut self) -> Result<Option<PlaybackSource>, PassFailure> {
        self.next_with_attempt(None).await
    }
    pub async fn next_with_attempt(
        &mut self,
        acquisition: Option<(u8, Instant)>,
    ) -> Result<Option<PlaybackSource>, PassFailure> {
        loop {
            tokio::select! {
                biased;
                (key, demand_id, result) = async { self.refreshing.as_mut().unwrap().done.as_mut().await }, if self.refreshing.is_some() => {
                    self.refreshing = None;
                    let current = self.key == Some(key)
                        && self.demand.as_ref().is_some_and(|demand| demand.id == demand_id);
                    if current {
                        self.candidates = None;
                        tracing::debug!(event = "player_sources_refreshed", success = result.is_ok(), selection = key.selection, "Provider refresh finished; reading current sources");
                        match result {
                            Err((PlayerError::Unauthorized, _)) => {
                                self.exhausted = true;
                                return Err(PassFailure { error: PlayerError::Unauthorized, retryable: false, cancelled: false });
                            }
                            Err((error, retryable)) => {
                                self.error = merged(self.error, error);
                                self.pass_retryable &= retryable;
                            }
                            Ok(_) => (),
                        }
                    } else {
                        tracing::debug!(event = "player_sources_refresh_retired", selection = key.selection, "Ignoring provider refresh from a retired demand");
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
                            return Err(PassFailure { error: self.error, retryable: false, cancelled: false });
                        },
                        Outcome::NoSuitable => {
                            self.exhausted = true;
                            return Err(PassFailure { error: self.error, retryable: self.pass_retryable, cancelled: false });
                        },
                        Outcome::ProviderUnavailable { retryable } => {
                            self.exhausted = true;
                            return Err(PassFailure { error: self.error, retryable: self.pass_retryable && retryable, cancelled: false });
                        },
                        Outcome::Cancelled => {
                            return Err(PassFailure { error: self.error, retryable: false, cancelled: true });
                        },
                        Outcome::ProtectedChoice => {
                            self.exhausted = true;
                            return Err(PassFailure { error: self.error, retryable: false, cancelled: false });
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
                            self.pass_retryable = false;
                            if matches!(error, PlayerError::Unauthorized | PlayerError::Unavailable) { self.exhausted = true; return Err(PassFailure { error: self.error, retryable: false, cancelled: false }); }
                        },
                    }
                    if self.refreshed {
                        if self.discovery_allowed
                            && self.error != PlayerError::OutputFormat
                            && self.candidates.as_ref().and_then(PlaybackCandidates::ended)
                                == Some(CandidatesEnd::Exhausted)
                        {
                            self.discovery_allowed = false;
                            let (attempt, started) = acquisition
                                .map_or((1, None), |(attempt, started)| {
                                    (attempt, Some(started))
                                });
                            self.discovering = Some(Discovery::start(
                                self.catalog.clone(),
                                self.demand.clone().unwrap(),
                                attempt,
                                started,
                            ));
                            continue;
                        }
                        self.exhausted = true;
                        tracing::debug!(event = "player_sources_exhausted", "Cached and refreshed sources exhausted");
                        let retryable = self.pass_retryable && self.error == PlayerError::Playback
                            && !matches!(self.candidates.as_ref().and_then(PlaybackCandidates::ended), Some(CandidatesEnd::FallbackOff | CandidatesEnd::AttemptLimit));
                        return Err(PassFailure { error: self.error, retryable, cancelled: false });
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
        sources.failed(PlayerError::Playback, false);
        assert!(sources.request(&library.actor, &library.source.recording_id));
        assert!(sources.next().await.is_err());
        assert!(sources.current().is_none());
    }

    #[tokio::test]
    async fn terminal_candidate_failure_stays_terminal_in_either_provider_order() {
        let library = AudioLibrary::start().await;
        let mut sources = Sources::new(library.hito.catalog.clone(), library.hito.playback.clone());
        sources.reset(Some(key()));
        sources.failed(PlayerError::Playback, false);
        sources.failed(PlayerError::Playback, true);
        assert!(!sources.pass_retryable);
        sources.reset(Some(key()));
        sources.failed(PlayerError::Playback, true);
        sources.failed(PlayerError::Playback, false);
        assert!(!sources.pass_retryable);
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
