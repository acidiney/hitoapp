use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SEARCH_PATH: &str = "v1/search";
pub const DETAIL_PATH: &str = "v1/recordings/detail";
pub const SOURCES_PATH: &str = "v1/sources";
pub const AUDIO_PATH: &str = "v1/audio";
pub const MAX_SOURCES_PER_RECORDING: usize = 16;

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    pub cursor: Option<String>,
    pub limit: u16,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SearchPage {
    pub items: Vec<ProviderRecording>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderRecording {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u64>,
    pub release: Option<ProviderRelease>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Box<crate::RecordingAttribution>>,
    /// ISO 3901 recording code when the provider states one. Hosts treat it as evidence, not identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// Canonical https pages of every credited performer. Hosts may match a lone page to one
    /// MusicBrainz artist; a provider that cannot name all performers omits the list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artist_urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artists: Vec<CreditedArtist>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CreditRole {
    #[default]
    Main,
    Featured,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CreditedArtist {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_artist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_url: Option<String>,
    #[serde(default)]
    pub role: CreditRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderRecordingRef {
    pub id: String,
    pub revision: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderRelease {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RecordingRequest {
    pub recording_id: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SourcePage {
    pub items: Vec<ProviderSource>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderSource {
    pub id: String,
    pub label: String,
    pub ready: bool,
}

impl RecordingRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.recording_id, 256)
    }
}

impl SourcePage {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.items.len() > MAX_SOURCES_PER_RECORDING {
            return Err("The provider returned too many sources");
        }
        let mut ids = HashSet::new();
        for source in &self.items {
            text(&source.id, 256)?;
            text(&source.label, 256)?;
            if !ids.insert(&source.id) {
                return Err("The provider returned duplicate source IDs");
            }
        }
        Ok(())
    }
}

impl SearchRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.query, 512)?;
        optional(self.cursor.as_deref(), 2048)?;
        if !(1..=50).contains(&self.limit) {
            return Err("Use a search limit between 1 and 50");
        }
        Ok(())
    }
}

impl SearchPage {
    pub fn validate(&self, requested: u16) -> Result<(), &'static str> {
        if self.items.len() > usize::from(requested.min(50)) {
            return Err("The provider returned too many search results");
        }
        optional(self.next_cursor.as_deref(), 2048)?;
        let mut ids = HashSet::new();
        for item in &self.items {
            item.validate()?;
            if !ids.insert(&item.id) {
                return Err("The provider returned duplicate recording IDs");
            }
        }
        Ok(())
    }
}

impl ProviderRecording {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.title, 2048)?;
        text(&self.artist, 2048)?;
        optional(self.revision.as_deref(), 256)?;
        if self
            .duration_ms
            .is_some_and(|duration| duration > 9_007_199_254_740_991)
        {
            return Err("The provider returned an unsupported recording duration");
        }
        if self.disc_number == Some(0) || self.track_number == Some(0) {
            return Err("Provider track and disc numbers must be positive");
        }
        if let Some(release) = &self.release {
            text(&release.id, 256)?;
            text(&release.title, 2048)?;
            optional(release.artist.as_deref(), 2048)?;
        }
        if let Some(attribution) = &self.attribution {
            attribution.validate()?;
        }
        if self.isrc.is_some() && self.normalized_isrc().is_none() {
            return Err("The provider returned a malformed ISRC");
        }
        validate_artist_urls(&self.artist_urls)?;
        validate_credited_artists(&self.artists)?;
        Ok(())
    }

    pub fn normalized_isrc(&self) -> Option<String> {
        self.isrc.as_deref().and_then(crate::normalize_isrc)
    }
}

impl ProviderRecordingRef {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        optional(self.revision.as_deref(), 256)
    }
}

const MAX_CREDITED_ARTISTS: usize = 16;
const MAX_ARTIST_NAME_BYTES: usize = 256;
const MAX_PROVIDER_ARTIST_ID_BYTES: usize = 128;
const MAX_JOIN_BYTES: usize = 32;

fn validate_credited_artists(artists: &[CreditedArtist]) -> Result<(), &'static str> {
    if artists.is_empty() {
        return Ok(());
    }
    if artists.len() > MAX_CREDITED_ARTISTS {
        return Err("The provider returned too many credited artists");
    }
    if !artists.iter().any(|artist| artist.role == CreditRole::Main) {
        return Err("The provider credited no main artist");
    }
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    let mut pages = HashSet::new();
    for artist in artists {
        text(&artist.name, MAX_ARTIST_NAME_BYTES)?;
        if !names.insert(artist.name.trim().to_lowercase()) {
            return Err("The provider credited the same artist twice");
        }
        if let Some(id) = &artist.provider_artist_id {
            text(id, MAX_PROVIDER_ARTIST_ID_BYTES)?;
            if id.chars().any(char::is_whitespace) || !ids.insert(id) {
                return Err("The provider returned a malformed credited artist identifier");
            }
        }
        optional(artist.join.as_deref(), MAX_JOIN_BYTES)?;
        if let Some(page) = &artist.page_url {
            validate_artist_page(page)?;
            if !pages.insert(page) {
                return Err("The provider credited the same artist page twice");
            }
        }
    }
    Ok(())
}

const MAX_ARTIST_URLS: usize = 8;
const MAX_ARTIST_URL_BYTES: usize = 512;

fn validate_artist_urls(urls: &[String]) -> Result<(), &'static str> {
    const MALFORMED: &str = "The provider returned a malformed artist page URL";
    if urls.len() > MAX_ARTIST_URLS {
        return Err("The provider returned too many artist page URLs");
    }
    let mut seen = HashSet::new();
    for value in urls {
        if validate_artist_page(value).is_err() || !seen.insert(value) {
            return Err(MALFORMED);
        }
    }
    Ok(())
}

fn validate_artist_page(value: &str) -> Result<(), &'static str> {
    let canonical = value.len() <= MAX_ARTIST_URL_BYTES
        && url::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.as_str() == value
        })
        && crate::artist_page_provider(value).is_some();
    if canonical {
        Ok(())
    } else {
        Err("The provider returned a malformed artist page URL")
    }
}

pub(crate) fn text(value: &str, maximum: usize) -> Result<(), &'static str> {
    if value.len() > maximum
        || value.trim().is_empty()
        || value
            .chars()
            .any(|character| character.is_control() || deceptive(character))
    {
        return Err("Provider fields must use bounded nonempty text without control characters");
    }
    Ok(())
}
pub(crate) fn optional(value: Option<&str>, maximum: usize) -> Result<(), &'static str> {
    value
        .map(|value| text(value, maximum))
        .transpose()
        .map(|_| ())
}

/// Invisible or reordering format characters that can disguise text. Joiners that scripts and
/// emoji sequences need (U+200C, U+200D) stay valid.
fn deceptive(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}
