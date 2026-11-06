mod candidates;
mod grant;
mod local;
pub(crate) mod origin;
mod preferred;
mod provider;
pub(crate) mod relay;

pub use candidates::{CandidatesEnd, MAX_FALLBACK_ATTEMPTS, PlaybackCandidates};
pub use grant::PlaybackSource;
pub use relay::{AudioContent, AudioRequest, AudioStream, ByteRange, OpenedAudio, StreamPace};

pub(crate) use local::LocalOrigin;
pub(crate) use origin::Origins;
pub(crate) use provider::ProviderOrigin;

use crate::{Actor, AudioFile, Error, HostDeviceId, Result, Store};
use std::time::Duration;

#[derive(Clone)]
pub struct PlaybackAccess {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) origins: Origins,
}

impl PlaybackAccess {
    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id()))]
    pub async fn playback_source(
        &self,
        actor: &Actor,
        recording: &str,
        source: &str,
    ) -> Result<PlaybackSource> {
        tokio::time::timeout(
            Duration::from_secs(1),
            PlaybackSource::admit(&self.store, &self.origins, actor, recording, source),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback authorization could not be checked in time",
        )))
    }

    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id()))]
    pub async fn open_audio(
        &self,
        actor: &Actor,
        source: &str,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        if let Some(opened) = self
            .origins
            .provider
            .open_current(actor, source, request)
            .await?
        {
            return Ok(opened);
        }
        self.source_grant(actor, source).await?.open(request).await
    }

    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id(), source_id))]
    pub async fn audio_file(&self, actor: &Actor, source: &str) -> Result<AudioFile> {
        actor.check_session(&self.store).await?;
        for origin in self.origins.iter() {
            if let Some(file) = origin.open_file(actor, source).await? {
                return Ok(file);
            }
        }
        Err(Error::NotFound)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id()))]
    async fn source_grant(&self, actor: &Actor, source: &str) -> Result<PlaybackSource> {
        tokio::time::timeout(Duration::from_secs(1), async {
            actor.check_session(&self.store).await?;
            let recording: String = sqlx::query_scalar(
                "SELECT recording_id FROM sources WHERE id=? AND provider IN ('local','download','podcast')",
            )
            .bind(source)
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)?;
            PlaybackSource::admit(&self.store, &self.origins, actor, &recording, source).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback authorization could not be checked in time",
        )))
    }
}
