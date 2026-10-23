use super::Catalog;
use super::audio::origin::{AudioOrigin, Located};
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
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playback_source(
        &self,
        actor: &Actor,
        recording: &str,
        source: &str,
    ) -> Result<PlaybackSource> {
        tokio::time::timeout(
            Duration::from_secs(1),
            self.admit_playback(actor, recording, source),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback authorization could not be checked in time",
        )))
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub(super) async fn audio_playback_source(
        &self,
        actor: &Actor,
        source: &str,
    ) -> Result<PlaybackSource> {
        tokio::time::timeout(Duration::from_secs(1), async {
            actor.check_session(&self.store).await?;
            let recording: String = sqlx::query_scalar(
                "SELECT recording_id FROM sources WHERE id=? AND provider IN ('local','download','podcast')",
            )
            .bind(source)
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)?;
            self.admit_playback(actor, &recording, source).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback authorization could not be checked in time",
        )))
    }

    async fn admit_playback(
        &self,
        actor: &Actor,
        recording: &str,
        source: &str,
    ) -> Result<PlaybackSource> {
        actor.check_session(&self.store).await?;
        let membership = playback_membership(&self.store, actor, recording, source).await?;
        let (origin, located) = self.origins.locate(actor, source).await?;
        let grant = PlaybackSource {
            id: NEXT_GRANT.fetch_add(1, Ordering::Relaxed),
            store: self.store.clone(),
            actor: actor.clone(),
            recording: recording.into(),
            source: source.into(),
            membership,
            origin,
            located,
            revoked: Arc::new(AtomicBool::new(false)),
        };
        grant.check().await?;
        tracing::debug!(
            playback_grant = grant.id,
            origin = grant.origin.kind(),
            "Playback source authorized"
        );
        Ok(grant)
    }
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
    pub fn source_id(&self) -> &str {
        &self.source
    }

    pub fn is_local(&self) -> bool {
        self.origin.local()
    }

    pub(super) fn actor(&self) -> &Actor {
        &self.actor
    }

    pub(super) fn located(&self) -> &Located {
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
    pub(super) async fn while_authorized<T>(
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

    pub(super) async fn supervised<T>(
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

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playback_grant = self.id))]
    pub async fn open(&self, request: AudioRequest) -> Result<OpenedAudio> {
        self.origin.open(self, request).await
    }
}
