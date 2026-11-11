use crate::{
    LyricsRequest, MAX_LYRICS_RESPONSE_BYTES, ProviderRecordingRef,
    content::{optional, text},
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};

pub const LYRICS_V2_CAPABILITIES_PATH: &str = "v2/lyrics/capabilities";
pub const LYRICS_V2_PATH: &str = "v2/lyrics";
pub const LYRICS_V2_VERSION: u32 = 2;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct LyricsCapabilitiesV2 {
    pub lyrics_version: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct LyricsResponseV2 {
    pub recording: ProviderRecordingRef,
    pub lyrics: Option<LyricsDocument>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct LyricsDocument {
    pub id: String,
    pub rendition_id: String,
    pub revision: Option<String>,
    pub rendition_label: Option<String>,
    pub language: Option<String>,
    pub duration_ms: Option<u64>,
    pub attribution: Option<LyricAttribution>,
    pub content: LyricContent,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LyricContent {
    Plain { lines: Vec<String> },
    Anchored { lines: Vec<AnchoredLyricLine> },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct AnchoredLyricLine {
    pub start_ms: u64,
    pub text: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct LyricAttribution {
    pub provider_label: String,
    pub content_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creators: Vec<String>,
}

impl LyricsCapabilitiesV2 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.lyrics_version == LYRICS_V2_VERSION {
            Ok(())
        } else {
            Err("Unsupported lyrics protocol version")
        }
    }
}

impl LyricsResponseV2 {
    pub fn validate_for(&self, request: &LyricsRequest) -> Result<(), &'static str> {
        request.validate()?;
        self.recording.validate()?;
        if self.recording != request.recording {
            return Err("The provider returned lyrics for a different recording version");
        }
        if let Some(document) = &self.lyrics {
            document.validate()?;
        }
        validate_encoded_size(self, MAX_LYRICS_RESPONSE_BYTES).map_err(|error| match error {
            EncodedSizeError::Exceeded => "The provider lyrics response exceeds the size limit",
            EncodedSizeError::Encoding => "The provider lyrics response could not be encoded",
        })
    }
}

impl LyricsDocument {
    fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.rendition_id, 256)?;
        optional(self.revision.as_deref(), 256)?;
        optional(self.rendition_label.as_deref(), 2048)?;
        optional(self.language.as_deref(), 64)?;
        if self
            .duration_ms
            .is_some_and(|duration| duration > 604_800_000)
        {
            return Err("The provider returned an unsupported lyrics duration");
        }
        if let Some(attribution) = &self.attribution {
            attribution.validate()?;
        }
        let count = match &self.content {
            LyricContent::Plain { lines } => lines.len(),
            LyricContent::Anchored { lines } => lines.len(),
        };
        if !(1..=2000).contains(&count) {
            return Err("The provider must return between one and two thousand lyric lines");
        }
        let mut has_text = false;
        match &self.content {
            LyricContent::Plain { lines } => {
                for line in lines {
                    has_text |= validate_line(line)?;
                }
            }
            LyricContent::Anchored { lines } => {
                let mut previous = 0;
                for line in lines {
                    has_text |= validate_line(&line.text)?;
                    if line.start_ms < previous
                        || line.start_ms > 604_800_000
                        || self
                            .duration_ms
                            .is_some_and(|duration| line.start_ms > duration)
                    {
                        return Err(
                            "Lyric anchors must be ordered within the known rendition duration and seven days",
                        );
                    }
                    previous = line.start_ms;
                }
            }
        }
        if !has_text {
            return Err("The provider lyric document must contain text");
        }
        Ok(())
    }
}

fn validate_line(value: &str) -> Result<bool, &'static str> {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return Err("Lyric lines must contain at most 2048 bytes without control characters");
    }
    Ok(!value.trim().is_empty())
}

impl LyricAttribution {
    fn validate(&self) -> Result<(), &'static str> {
        text(&self.provider_label, 256)?;
        if self.creators.len() > 16 {
            return Err("Declare at most sixteen known lyric creators");
        }
        for creator in &self.creators {
            text(creator, 2048)?;
        }
        let value = &self.content_url;
        const INVALID: &str =
            "Lyric backlinks must use canonical HTTPS without credentials, query, or fragment";
        if value.len() > 2048
            || value.contains('\\')
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(INVALID);
        }
        let url = url::Url::parse(value).map_err(|_| INVALID)?;
        if url.scheme() != "https"
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.as_str() != value
        {
            return Err(INVALID);
        }
        Ok(())
    }
}
