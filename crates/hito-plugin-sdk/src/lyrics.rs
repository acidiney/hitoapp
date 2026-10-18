use crate::{
    ProviderRecordingRef, RecordingAttribution,
    content::{optional, text},
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};

pub const LYRICS_CAPABILITIES_PATH: &str = "v1/lyrics/capabilities";
pub const LYRICS_PATH: &str = "v1/lyrics";
pub const LYRICS_VERSION: u32 = 1;
pub const MAX_LYRICS_RESPONSE_BYTES: usize = 262_144;
const MAX_DURATION_MS: u64 = 604_800_000;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct LyricsCapabilities {
    pub lyrics_version: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct LyricsRequest {
    pub recording: ProviderRecordingRef,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct LyricsResponse {
    pub recording: ProviderRecordingRef,
    pub lyrics: Option<TimedLyrics>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct TimedLyrics {
    pub id: String,
    pub rendition_id: String,
    pub revision: Option<String>,
    pub rendition_label: Option<String>,
    pub language: Option<String>,
    pub duration_ms: Option<u64>,
    pub attribution: Option<RecordingAttribution>,
    pub lines: Vec<TimedLyricLine>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct TimedLyricLine {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl LyricsCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.lyrics_version != LYRICS_VERSION {
            return Err("Unsupported lyrics protocol version");
        }
        Ok(())
    }
}

impl LyricsRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.recording.validate()
    }
}

impl LyricsResponse {
    pub fn validate_for(&self, request: &LyricsRequest) -> Result<(), &'static str> {
        request.validate()?;
        self.recording.validate()?;
        if self.recording != request.recording {
            return Err("The provider returned lyrics for a different recording version");
        }
        if let Some(lyrics) = &self.lyrics {
            lyrics.validate()?;
        }
        validate_encoded_size(self, MAX_LYRICS_RESPONSE_BYTES).map_err(|error| match error {
            EncodedSizeError::Exceeded => "The provider lyrics response exceeds the size limit",
            EncodedSizeError::Encoding => "The provider lyrics response could not be encoded",
        })?;
        Ok(())
    }
}

impl TimedLyrics {
    fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.rendition_id, 256)?;
        optional(self.revision.as_deref(), 256)?;
        optional(self.rendition_label.as_deref(), 2048)?;
        optional(self.language.as_deref(), 64)?;
        if let Some(attribution) = &self.attribution {
            attribution.validate()?;
        }
        if self
            .duration_ms
            .is_some_and(|duration| duration > MAX_DURATION_MS)
        {
            return Err("The provider returned an unsupported lyrics duration");
        }
        if self.lines.is_empty() || self.lines.len() > 2000 {
            return Err("The provider must return between one and two thousand timed lyric lines");
        }
        let mut previous_end = 0;
        for line in &self.lines {
            text(&line.text, 2048)?;
            if line.start_ms >= line.end_ms
                || line.end_ms > MAX_DURATION_MS
                || line.start_ms < previous_end
            {
                return Err(
                    "Timed lyric lines must use ordered nonoverlapping intervals within seven days",
                );
            }
            if self
                .duration_ms
                .is_some_and(|duration| line.end_ms > duration)
            {
                return Err("Timed lyric lines must end within the known rendition duration");
            }
            previous_end = line.end_ms;
        }
        Ok(())
    }
}
