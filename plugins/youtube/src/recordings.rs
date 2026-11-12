use crate::Error;
use hito_plugin_sdk::{CreditRole, CreditedArtist, ProviderRecording};
use serde::Deserialize;
use std::collections::HashSet;

const MAX_CREDITED_ARTISTS: usize = 16;
const MAX_CREDITED_NAME_BYTES: usize = 256;

fn main_credits(names: &[String]) -> Vec<CreditedArtist> {
    let mut distinct = HashSet::new();
    let creditable = names.len() <= MAX_CREDITED_ARTISTS
        && names.iter().all(|name| {
            name.len() <= MAX_CREDITED_NAME_BYTES && distinct.insert(name.trim().to_lowercase())
        });
    if !creditable {
        return Vec::new();
    }
    names
        .iter()
        .enumerate()
        .map(|(index, name)| CreditedArtist {
            name: name.trim().to_owned(),
            provider_artist_id: None,
            page_url: None,
            role: CreditRole::Main,
            join: (index + 1 < names.len()).then(|| ", ".to_owned()),
        })
        .collect()
}

#[derive(Clone, Deserialize)]
pub(crate) struct SearchEntry {
    id: String,
    title: String,
    artists: Option<Vec<String>>,
    artist: Option<String>,
    channel: Option<String>,
    uploader: Option<String>,
    duration: Option<f64>,
}

pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

impl SearchEntry {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn enriched(self, video: Self) -> Self {
        let artists = self.artists.filter(|artists| !artists.is_empty());
        let musical = artists.is_some() || self.artist.is_some();
        Self {
            id: self.id,
            title: self.title,
            artists: if musical { artists } else { video.artists },
            artist: if musical { self.artist } else { video.artist },
            channel: self.channel.or(video.channel),
            uploader: self.uploader.or(video.uploader),
            duration: self.duration.or(video.duration),
        }
    }

    pub(crate) fn summary(mut self) -> Result<ProviderRecording, Error> {
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
        let mut credited = Vec::new();
        let artist = match entry.artists {
            Some(artists) if !artists.is_empty() => {
                if artists
                    .iter()
                    .any(|artist| artist.trim().is_empty() || artist.chars().any(char::is_control))
                {
                    return Err(Error::InvalidResponse);
                }
                credited = main_credits(&artists);
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
            isrc: None,
            artist_urls: Vec::new(),
            artists: credited,
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
