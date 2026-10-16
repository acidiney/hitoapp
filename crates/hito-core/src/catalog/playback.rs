use super::{Catalog, media::Location};
use crate::{Actor, AudioFile, AudioRequest, Error, OpenedAudio, Result};
use std::{
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
    catalog: Catalog,
    actor: Actor,
    recording: String,
    source: String,
    membership: String,
    binding: Binding,
    revoked: Arc<AtomicBool>,
}

#[derive(Clone)]
enum Binding {
    Local(Location),
    Provider(crate::plugins::AudioAuthority),
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

    async fn admit_playback(
        &self,
        actor: &Actor,
        recording: &str,
        source: &str,
    ) -> Result<PlaybackSource> {
        actor.check_session(&self.store).await?;
        let membership = self.playback_membership(actor, recording, source).await?;
        let binding = if let Some((operation, remote)) = self.provider_audio(actor, source).await? {
            Binding::Provider(operation.audio_authority(source, &remote))
        } else {
            Binding::Local(self.media_location(actor, source).await?)
        };
        let grant = PlaybackSource {
            id: NEXT_GRANT.fetch_add(1, Ordering::Relaxed),
            catalog: self.clone(),
            actor: actor.clone(),
            recording: recording.into(),
            source: source.into(),
            membership,
            binding,
            revoked: Arc::new(AtomicBool::new(false)),
        };
        grant.check().await?;
        tracing::debug!(playback_grant = grant.id, "Playback source authorized");
        Ok(grant)
    }

    async fn playback_membership(
        &self,
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
        .fetch_optional(&self.store.pool)
        .await?
        .ok_or(Error::NotFound)
    }
}

impl PlaybackSource {
    pub fn is_local(&self) -> bool {
        matches!(self.binding, Binding::Local(_))
    }

    #[tracing::instrument(level = "trace", skip_all, err(Debug), fields(playback_grant = self.id))]
    pub async fn check(&self) -> Result<()> {
        if self.revoked.load(Ordering::Acquire) {
            return Err(Error::NotFound);
        }
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            self.actor.check_session(&self.catalog.store).await?;
            if self
                .catalog
                .playback_membership(&self.actor, &self.recording, &self.source)
                .await?
                != self.membership
            {
                return Err(Error::NotFound);
            }
            match &self.binding {
                Binding::Local(location)
                    if self
                        .catalog
                        .media_location(&self.actor, &self.source)
                        .await?
                        != *location =>
                {
                    return Err(Error::NotFound);
                }
                Binding::Provider(authority) => authority.check().await?,
                _ => {}
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

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playback_grant = self.id))]
    pub async fn open_file(&self) -> Result<AudioFile> {
        self.check().await?;
        if !matches!(self.binding, Binding::Local(_)) {
            return Err(Error::NotFound);
        }
        let file = tokio::select! {
            biased;
            error = self.revoked() => return Err(error),
            result = self.catalog.audio_file(&self.actor, &self.source) => result?,
        };
        self.check().await?;
        Ok(file)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playback_grant = self.id))]
    pub async fn open(&self, request: AudioRequest) -> Result<OpenedAudio> {
        self.check().await?;
        let opened = tokio::select! {
            biased;
            error = self.revoked() => return Err(error),
            result = self.catalog.open_audio(&self.actor, &self.source, request) => result?,
        };
        self.check().await?;
        Ok(opened)
    }
}
