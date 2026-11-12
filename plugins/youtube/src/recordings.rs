use crate::Error;
use hito_plugin_sdk::{CreditRole, CreditedArtist, ProviderRecording, SearchPage};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const MAX_RESPONSE_BYTES: usize = 262_144;
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
    let mut documents = serde_json::Deserializer::from_slice(bytes).into_iter::<SearchResults>();
    let videos = documents
        .next()
        .ok_or(Error::InvalidResponse)?
        .map_err(|_| Error::InvalidResponse)?;
    let songs = documents
        .next()
        .transpose()
        .map_err(|_| Error::InvalidResponse)?
        .map_or_else(Vec::new, |results| results.entries);
    if documents.next().is_some() {
        return Err(Error::InvalidResponse);
    }
    let mut items = interleaved(summaries(videos.entries)?, summaries(songs)?);
    items.truncate(usize::from(limit));
    let page = SearchPage {
        items,
        next_cursor: None,
    };
    page.validate(limit).map_err(|_| Error::InvalidResponse)?;
    let length = serde_json::to_vec(&page)
        .map_err(|_| Error::InvalidResponse)?
        .len();
    if length > MAX_RESPONSE_BYTES {
        return Err(Error::InvalidResponse);
    }
    Ok(page)
}

fn summaries(entries: Vec<SearchEntry>) -> Result<Vec<ProviderRecording>, Error> {
    if entries.len() > 50 {
        return Err(Error::InvalidResponse);
    }
    entries.into_iter().map(SearchEntry::summary).collect()
}

fn interleaved(
    videos: Vec<ProviderRecording>,
    songs: Vec<ProviderRecording>,
) -> Vec<ProviderRecording> {
    let mut described: HashMap<String, ProviderRecording> = videos
        .iter()
        .map(|video| (video.id.clone(), video.clone()))
        .collect();
    let mut emitted = HashSet::new();
    let mut ranked = Vec::with_capacity(videos.len() + songs.len());
    let (mut videos, mut songs) = (videos.into_iter(), songs.into_iter());
    loop {
        let (video, song) = (videos.next(), songs.next());
        if video.is_none() && song.is_none() {
            return ranked;
        }
        for candidate in [video, song].into_iter().flatten() {
            if emitted.insert(candidate.id.clone()) {
                ranked.push(described.remove(&candidate.id).unwrap_or(candidate));
            }
        }
    }
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
