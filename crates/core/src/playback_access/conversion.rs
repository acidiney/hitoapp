mod limits;
mod process;
mod profile;

use crate::{Actor, AudioRequest, Error, MediaTools, OpenedAudio, PlaybackSource, Result};
pub(crate) use limits::{ConversionLimits, ConversionSlot};
pub use process::ConversionStream;
pub(crate) use process::{Input, PositionedAudio};
use std::time::{Duration, Instant};

const CONTENT_TYPE: &str = "audio/aac";
pub const MAX_CONVERSION_START: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    serde::Deserialize,
    serde::Serialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum StreamQuality {
    #[default]
    Original,
    High,
    #[serde(rename = "saver")]
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
    pub body: Option<ConvertedBody>,
}

pub enum ConvertedBody {
    Direct(Box<ConversionStream>),
    Lease(crate::AudioStream),
}

impl ConvertedBody {
    pub async fn chunk(&mut self) -> Result<Option<bytes::Bytes>> {
        match self {
            Self::Direct(stream) => stream.chunk().await,
            Self::Lease(stream) => stream.chunk().await,
        }
    }
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

pub(crate) enum Negotiated {
    Converted(Option<Box<ConversionStream>>),
    Unchanged(&'static str),
}

pub(super) async fn negotiate(
    limits: &ConversionLimits,
    tools: &MediaTools,
    actor: &Actor,
    grant: &PlaybackSource,
    input: Input<'_>,
    request: QualityRequest,
    target: u32,
) -> Result<Negotiated> {
    let slot = limits.acquire(actor.id())?;
    let started = Instant::now();
    let unchanged = |reason: &'static str| {
        tracing::debug!(
            event = "audio_conversion_skipped",
            quality = request.quality.label(),
            reason,
            "Audio is streamed unchanged"
        );
        Ok(Negotiated::Unchanged(reason))
    };
    let (profile, slot) = match profile::probe(tools, input, grant.clone(), slot).await {
        Ok(profile) => profile,
        Err(error) => {
            tracing::warn!(
                event = "audio_conversion_failed",
                stage = "probe",
                error_kind = error.code(),
                "Audio could not be inspected for conversion"
            );
            return unchanged("conversion-failed");
        }
    };
    grant.check().await?;
    if let Some(reason) = profile.passthrough_reason(target) {
        drop(slot);
        return unchanged(reason);
    }
    if request.original.head {
        return Ok(Negotiated::Converted(None));
    }
    match process::start(
        tools,
        input,
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
            Ok(Negotiated::Converted(Some(Box::new(stream))))
        }
        Err(error) => {
            tracing::warn!(
                event = "audio_conversion_failed",
                stage = "start",
                error_kind = error.code(),
                "Audio conversion did not start"
            );
            unchanged("conversion-failed")
        }
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
    match negotiate(
        limits,
        tools,
        actor,
        &grant,
        Input::Descriptor(&file),
        request,
        target,
    )
    .await?
    {
        Negotiated::Converted(stream) => Ok(converted(
            request,
            target,
            stream.map(ConvertedBody::Direct),
        )),
        Negotiated::Unchanged(reason) => original(&grant, request, reason).await,
    }
}

pub(super) fn converted(
    request: QualityRequest,
    target: u32,
    body: Option<ConvertedBody>,
) -> QualityAudio {
    QualityAudio::Converted(Box::new(ConvertedAudio {
        content_type: CONTENT_TYPE,
        bitrate: target,
        start: request.start,
        body,
    }))
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
