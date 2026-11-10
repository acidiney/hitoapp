mod limits;
mod process;
mod profile;

use crate::{Actor, AudioRequest, Error, MediaTools, OpenedAudio, PlaybackSource, Result};
pub(crate) use limits::{ConversionLimits, ConversionSlot};
pub use process::ConversionStream;
use std::time::{Duration, Instant};

const CONTENT_TYPE: &str = "audio/aac";
pub const MAX_CONVERSION_START: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamQuality {
    #[default]
    Original,
    High,
    DataSaver,
}

impl StreamQuality {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "original" => Some(Self::Original),
            "high" => Some(Self::High),
            "saver" => Some(Self::DataSaver),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::High => "high",
            Self::DataSaver => "saver",
        }
    }

    pub fn target_bitrate(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::High => Some(256_000),
            Self::DataSaver => Some(128_000),
        }
    }
}

#[derive(Clone, Copy)]
pub struct QualityRequest {
    pub quality: StreamQuality,
    pub start: Duration,
    pub original: AudioRequest,
}

pub struct ConvertedAudio {
    pub content_type: &'static str,
    pub bitrate: u32,
    pub start: Duration,
    pub body: Option<ConversionStream>,
}

pub enum QualityAudio {
    Converted(Box<ConvertedAudio>),
    Original {
        audio: OpenedAudio,
        reason: &'static str,
    },
}

impl QualityRequest {
    pub fn validate(&self) -> Result<()> {
        if self.start > MAX_CONVERSION_START {
            return Err(Error::Invalid("Start position is out of range".into()));
        }
        Ok(())
    }
}

pub(super) async fn convert(
    limits: &ConversionLimits,
    tools: &MediaTools,
    actor: &Actor,
    grant: PlaybackSource,
    file: crate::AudioFile,
    request: QualityRequest,
    target: u32,
) -> Result<QualityAudio> {
    let slot = limits.acquire(actor.id())?;
    let started = Instant::now();
    let fallback = |reason: &'static str| {
        tracing::debug!(
            event = "audio_conversion_skipped",
            quality = request.quality.label(),
            reason,
            "Audio is streamed unchanged"
        );
    };
    let (profile, slot) = match profile::probe(tools, &file, grant.clone(), slot).await {
        Ok(profile) => profile,
        Err(error) => {
            tracing::warn!(
                event = "audio_conversion_failed",
                stage = "probe",
                error_kind = error.code(),
                "Audio could not be inspected for conversion"
            );
            fallback("conversion-failed");
            return original(&grant, request, "conversion-failed").await;
        }
    };
    grant.check().await?;
    if let Some(reason) = profile.passthrough_reason(target) {
        fallback(reason);
        drop(slot);
        return original(&grant, request, reason).await;
    }
    let converted = |body| {
        QualityAudio::Converted(Box::new(ConvertedAudio {
            content_type: CONTENT_TYPE,
            bitrate: target,
            start: request.start,
            body,
        }))
    };
    if request.original.head {
        return Ok(converted(None));
    }
    match process::start(
        tools,
        &file,
        request.quality,
        target,
        request.start,
        grant.clone(),
        slot,
    )
    .await
    {
        Ok(stream) => {
            tracing::debug!(
                event = "audio_conversion_started",
                quality = request.quality.label(),
                bitrate = target,
                startup_ms = started.elapsed().as_millis() as u64,
                "Audio conversion started"
            );
            Ok(converted(Some(stream)))
        }
        Err(error) => {
            tracing::warn!(
                event = "audio_conversion_failed",
                stage = "start",
                error_kind = error.code(),
                "Audio conversion did not start"
            );
            original(&grant, request, "conversion-failed").await
        }
    }
}

pub(super) async fn original(
    grant: &PlaybackSource,
    request: QualityRequest,
    reason: &'static str,
) -> Result<QualityAudio> {
    Ok(QualityAudio::Original {
        audio: grant.open(request.original).await?,
        reason,
    })
}
