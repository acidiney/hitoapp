use super::{
    PlaybackAccess,
    decoding::DecodeContext,
    origin::{AudioOrigin, Located},
};
use crate::{Actor, AudioFile, AudioRequest, Error, OpenedAudio, Result, Store};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

static NEXT_GRANT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct PlaybackSource {
    id: u64,
    store: Store,
    actor: Actor,
    recording: String,
    source: String,
    membership: String,
    origin: Arc<dyn AudioOrigin>,
    located: Located,
    revoked: Arc<AtomicBool>,
    pub(super) decoding: DecodeContext,
}

async fn playback_membership(
    store: &Store,
    actor: &Actor,
    recording: &str,
    source: &str,
) -> Result<String> {
    sqlx::query_scalar(
        "SELECT access.id FROM sources s JOIN recordings r ON r.id=s.recording_id
        JOIN library_access_grants access ON access.library_id=r.library_id AND access.user_id=?
        WHERE s.id=? AND r.id=?",
    )
    .bind(actor.id())
    .bind(source)
    .bind(recording)
    .fetch_optional(&store.pool)
    .await?
    .ok_or(Error::NotFound)
}

impl PlaybackSource {
    pub(super) fn admit(
        access: &PlaybackAccess,
        actor: &Actor,
        recording: &str,
        source: &str,
    ) -> futures_util::future::BoxFuture<'static, Result<Self>> {
        let store = access.store.clone();
        let origins = access.origins.clone();
        let actor = actor.clone();
        let recording = recording.to_owned();
        let source = source.to_owned();
        let decoding = DecodeContext {
            tools: access.tools.clone(),
            limits: access.conversions.clone(),
        };
        Box::pin(async move {
            actor.check_session(&store).await?;
            let membership = playback_membership(&store, &actor, &recording, &source).await?;
            let (origin, located) = origins.locate(&actor, &source).await?;
            let grant = Self {
                id: NEXT_GRANT.fetch_add(1, Ordering::Relaxed),
                store,
                actor,
                recording,
                source,
                membership,
                origin,
                located,
                revoked: Arc::new(AtomicBool::new(false)),
                decoding,
            };
            grant.check().await?;
            tracing::debug!(
                playback_grant = grant.id,
                origin = grant.origin.kind(),
                "Playback source authorized"
            );
            Ok(grant)
        })
    }

    pub(crate) fn correlation_id(&self) -> u64 {
        self.id
    }

    pub fn source_id(&self) -> &str {
        &self.source
    }

    pub fn is_local(&self) -> bool {
        self.origin.local()
    }

    pub(crate) fn is_provider_remote(&self) -> bool {
        self.origin.kind() == "provider" && !self.origin.local()
    }

    pub(crate) async fn warm_open(
        &self,
        request: AudioRequest,
        monitors: Arc<crate::playback_access::relay::MonitorRegistry>,
    ) -> Result<(OpenedAudio, Arc<crate::plugins::WarmTicket>)> {
        self.warm_check().await?;
        self.origin
            .open_warm(self, request, monitors)
            .await?
            .ok_or(Error::NotFound)
    }

    pub(crate) async fn warm_check(&self) -> Result<()> {
        if self.revoked.load(Ordering::Acquire) {
            return Err(Error::NotFound);
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            self.actor.check_session(&self.store).await?;
            if playback_membership(&self.store, &self.actor, &self.recording, &self.source).await?
                != self.membership
            {
                return Err(Error::NotFound);
            }
            if !self
                .origin
                .warm_still_valid(&self.actor, &self.source, &self.located)
                .await?
            {
                return Err(Error::NotFound);
            }
            Ok(())
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback authorization could not be checked in time",
        )))
    }

    pub(crate) fn warm_equivalent(&self, other: &Self) -> bool {
        if !self.actor.same_session(&other.actor)
            || self.recording != other.recording
            || self.source != other.source
            || self.membership != other.membership
            || self.origin.kind() != other.origin.kind()
        {
            return false;
        }
        let (Ok(left), Ok(right)) = (
            self.located.pinned::<crate::plugins::AudioAuthority>(),
            other.located.pinned::<crate::plugins::AudioAuthority>(),
        ) else {
            return false;
        };
        left.same_identity(right)
    }

    pub(crate) fn actor(&self) -> &Actor {
        &self.actor
    }

    pub(crate) fn located(&self) -> &Located {
        &self.located
    }

    #[tracing::instrument(level = "trace", skip_all, err(Debug), fields(playback_grant = self.id))]
    pub async fn check(&self) -> Result<()> {
        if self.revoked.load(Ordering::Acquire) {
            return Err(Error::NotFound);
        }
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            self.actor.check_session(&self.store).await?;
            if playback_membership(&self.store, &self.actor, &self.recording, &self.source).await?
                != self.membership
            {
                return Err(Error::NotFound);
            }
            if !self
                .origin
                .still_valid(&self.actor, &self.source, &self.located)
                .await?
            {
                return Err(Error::NotFound);
            }
            Ok(())
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback authorization could not be checked in time",
        )));
        if let Err(error) = &result
            && !self.revoked.swap(true, Ordering::AcqRel)
        {
            tracing::debug!(
                error_kind = error.code(),
                "Playback source authorization revoked"
            );
        }
        result?;
        if self.revoked.load(Ordering::Acquire) {
            Err(Error::NotFound)
        } else {
            Ok(())
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(playback_grant = self.id))]
    pub async fn revoked(&self) -> Error {
        loop {
            if let Err(error) = self.check().await {
                return error;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    #[tracing::instrument(level = "trace", skip_all, err(Debug), fields(playback_grant = self.id))]
    pub(crate) async fn while_authorized<T>(
        &self,
        operation: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        self.check().await?;
        let result = tokio::select! {
            biased;
            error = async {
                tokio::time::sleep(Duration::from_millis(250)).await;
                self.revoked().await
            } => Err(error),
            result = tokio::time::timeout(Duration::from_secs(15), operation) => {
                result.unwrap_or(Err(Error::Upstream(
                    "Audio operation did not finish in time",
                )))
            },
        };
        self.check().await?;
        result
    }

    pub(crate) async fn supervised<T>(
        &self,
        operation: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        self.check().await?;
        let value = tokio::select! {
            biased;
            error = self.revoked() => return Err(error),
            result = operation => result?,
        };
        self.check().await?;
        Ok(value)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playback_grant = self.id))]
    pub async fn open_file(&self) -> Result<AudioFile> {
        self.supervised(async {
            self.origin
                .open_file(&self.actor, &self.source)
                .await?
                .ok_or(Error::NotFound)
        })
        .await
    }

    pub(crate) async fn open_file_if_present(&self) -> Result<Option<AudioFile>> {
        self.supervised(self.origin.open_file(&self.actor, &self.source))
            .await
    }

    #[tracing::instrument(level = "debug", skip_all, err(level = "debug", Debug), fields(playback_grant = self.id))]
    pub async fn open(&self, request: AudioRequest) -> Result<OpenedAudio> {
        self.origin.open(self, request).await
    }
}

impl super::relay::remote::AuthorityCheck for PlaybackSource {
    fn check(&self) -> futures_util::future::BoxFuture<'_, Result<()>> {
        Box::pin(PlaybackSource::check(self))
    }
}
