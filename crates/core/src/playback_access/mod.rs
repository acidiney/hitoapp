mod candidates;
mod conversion;
mod decoding;
mod grant;
mod leases;
mod local;
pub(crate) mod origin;
mod preferred;
mod provider;
pub(crate) mod relay;
mod warm;

pub use candidates::{CandidatesEnd, MAX_FALLBACK_ATTEMPTS, PlaybackCandidates};
pub use conversion::{
    ConversionStream, ConvertedAudio, ConvertedBody, MAX_CONVERSION_START, QualityAudio,
    QualityRequest, StreamQuality,
};
pub use decoding::{PcmBlock, PcmFormat, PcmInput, PcmOpening, PcmStream, StreamDecoder};
pub use grant::PlaybackSource;
pub use leases::{BeginPlaybackLease, PlaybackLease, ReleaseClientLeases};
pub use relay::{
    AudioContent, AudioRequest, AudioStream, AudioValidators, ByteRange, OpenedAudio, StreamPace,
};

pub(crate) use conversion::ConversionLimits;
pub(crate) use local::LocalOrigin;
pub(crate) use origin::Origins;
pub(crate) use provider::ProviderOrigin;

use crate::{Actor, AudioFile, Error, HostDeviceId, MediaTools, Result, Store};
use std::time::Duration;

#[derive(Clone)]
pub struct PlaybackAccess {
    pub(crate) leases: std::sync::Arc<leases::registry::Registry>,
    pub(crate) warm: std::sync::Arc<warm::Registry>,
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) origins: Origins,
    pub(crate) tools: MediaTools,
    pub(crate) conversions: conversion::ConversionLimits,
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
            PlaybackSource::admit(self, actor, recording, source),
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
        let grant = self.source_grant(actor, source).await?;
        self.open_admitted_audio(actor, &grant, request).await
    }

    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id()))]
    pub async fn open_quality_audio(
        &self,
        actor: &Actor,
        source: &str,
        request: QualityRequest,
    ) -> Result<QualityAudio> {
        request.validate()?;
        let Some(target) = request.quality.target_bitrate() else {
            return self
                .original_audio(actor, source, request, "original-requested")
                .await;
        };
        let grant = self.source_grant(actor, source).await?;
        if grant.is_provider_remote() {
            return Ok(QualityAudio::Original {
                audio: self
                    .open_admitted_audio(actor, &grant, request.original)
                    .await?,
                reason: "remote-source",
            });
        }
        let Some(file) = grant.open_file_if_present().await? else {
            return conversion::original(&grant, request, "remote-source").await;
        };
        conversion::convert(
            &self.conversions,
            &self.tools,
            actor,
            grant,
            file,
            request,
            target,
        )
        .await
    }

    async fn original_audio(
        &self,
        actor: &Actor,
        source: &str,
        request: QualityRequest,
        reason: &'static str,
    ) -> Result<QualityAudio> {
        Ok(QualityAudio::Original {
            audio: self.open_audio(actor, source, request.original).await?,
            reason,
        })
    }

    async fn open_admitted_audio(
        &self,
        actor: &Actor,
        grant: &PlaybackSource,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        if grant.is_provider_remote()
            && let Some(mut taken) = self.warm.claim_source(actor, grant, request).await?
        {
            let expected = taken.content.length;
            taken.content.body = Some(AudioStream::prepared(
                taken.prefix,
                taken.body,
                taken.reservation,
                expected,
            ));
            return Ok(OpenedAudio::Content(taken.content));
        }
        grant.open(request).await
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
                "SELECT recording_id FROM sources WHERE id=? AND provider IN ('local','download','podcast','plugin')",
            )
            .bind(source)
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)?;
            PlaybackSource::admit(self, actor, &recording, source).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback authorization could not be checked in time",
        )))
    }
}
