use crate::Error;
use hito_plugin_sdk::{ProviderRecording, SearchPage};
use serde::Deserialize;

const MAX_RESPONSE_BYTES: usize = 262_144;

#[derive(Deserialize)]
struct SearchResults {
    entries: Vec<SearchEntry>,
}

#[derive(Deserialize)]
pub(crate) struct SearchEntry {
    id: String,
    title: String,
    artists: Option<Vec<String>>,
    artist: Option<String>,
    channel: Option<String>,
    uploader: Option<String>,
    duration: Option<f64>,
}

pub(crate) fn normalize(bytes: &[u8], limit: u16) -> Result<SearchPage, Error> {
    let results: SearchResults =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    if results.entries.len() > 50 {
        return Err(Error::InvalidResponse);
    }
    let mut items = Vec::with_capacity(results.entries.len());
    for entry in results.entries {
        items.push(entry.summary()?);
    }
    let mut page = SearchPage {
        items,
        next_cursor: None,
    };
    page.validate(50).map_err(|_| Error::InvalidResponse)?;
    page.items.truncate(usize::from(limit));
    let length = serde_json::to_vec(&page)
        .map_err(|_| Error::InvalidResponse)?
        .len();
    if length > MAX_RESPONSE_BYTES {
        return Err(Error::InvalidResponse);
    }
    Ok(page)
}

pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

impl SearchEntry {
    fn summary(mut self) -> Result<ProviderRecording, Error> {
        if self.artists.as_ref().is_none_or(Vec::is_empty)
            && self.artist.is_none()
            && self.channel.is_none()
            && self.uploader.is_none()
        {
            self.artist = Some("Unknown artist".into());
        }
        self.recording()
    }

    pub(crate) fn recording(self) -> Result<ProviderRecording, Error> {
        let entry = self;
        if !valid_id(&entry.id) {
            return Err(Error::InvalidResponse);
        }
        let artist = match entry.artists {
            Some(artists) if !artists.is_empty() => {
                if artists
                    .iter()
                    .any(|artist| artist.trim().is_empty() || artist.chars().any(char::is_control))
                {
                    return Err(Error::InvalidResponse);
                }
                artists.join(", ")
            }
            _ => match entry.artist {
                Some(artist) => artist,
                None => {
                    let channel = entry
                        .channel
                        .or(entry.uploader)
                        .ok_or(Error::InvalidResponse)?;
                    if channel.trim().is_empty() || channel.chars().any(char::is_control) {
                        return Err(Error::InvalidResponse);
                    }
                    format!("Channel: {channel}")
                }
            },
        };
        let duration_ms = entry
            .duration
            .map(|duration| {
                let milliseconds = (duration * 1000.0).round();
                if !duration.is_finite() || duration < 0.0 || milliseconds > 9_007_199_254_740_991.0
                {
                    return Err(Error::InvalidResponse);
                }
                Ok(milliseconds as u64)
            })
            .transpose()?;
        let recording = ProviderRecording {
            id: entry.id,
            title: entry.title,
            artist,
            duration_ms,
            release: None,
            disc_number: None,
            track_number: None,
            revision: None,
            attribution: None,
        };
        recording.validate().map_err(|_| Error::InvalidResponse)?;
        Ok(recording)
    }
}
