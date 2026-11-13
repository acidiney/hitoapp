use crate::{Error, process, recordings::SearchEntry};
use hito_plugin_sdk::SearchPage;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const MAX_RESPONSE_BYTES: usize = 262_144;
const MAX_ENTRIES: usize = 50;
const MAX_DOCUMENTS: usize = 2;

#[derive(Deserialize)]
struct Document {
    entries: Vec<SearchEntry>,
    original_url: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Origin {
    Songs,
    Videos,
}

#[derive(Default)]
struct Results {
    songs: Vec<SearchEntry>,
    videos: Vec<SearchEntry>,
}

pub(crate) fn normalize(bytes: &[u8], query: &str, limit: u16) -> Result<SearchPage, Error> {
    let results = documents(bytes, query, limit)?;
    let items = ranked(results, query)
        .into_iter()
        .take(usize::from(limit))
        .map(SearchEntry::summary)
        .collect::<Result<Vec<_>, _>>()?;
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

fn documents(bytes: &[u8], query: &str, limit: u16) -> Result<Results, Error> {
    let songs_target = process::songs_target(query)?;
    let videos_target = process::videos_target(query, limit);
    let mut results = Results::default();
    let mut seen = Vec::new();
    let mut usable = false;
    let frames = frames(bytes);
    if frames.len() > MAX_DOCUMENTS {
        return Err(Error::InvalidResponse);
    }
    for value in frames.into_iter().flatten() {
        let Ok(document) = serde_json::from_value::<Document>(value) else {
            continue;
        };
        let origin = match document.original_url.as_deref() {
            None => Origin::Songs,
            Some(url) if url == songs_target => Origin::Songs,
            Some(url) if url == videos_target => Origin::Videos,
            Some(_) => continue,
        };
        if seen.contains(&origin) {
            return Err(Error::InvalidResponse);
        }
        seen.push(origin);
        if document.entries.len() > MAX_ENTRIES
            || document
                .entries
                .iter()
                .any(|entry| entry.clone().summary().is_err())
        {
            continue;
        }
        usable = true;
        match origin {
            Origin::Songs => results.songs = document.entries,
            Origin::Videos => results.videos = document.entries,
        }
    }
    if usable {
        Ok(results)
    } else {
        Err(Error::InvalidResponse)
    }
}

fn frames(bytes: &[u8]) -> Vec<Option<serde_json::Value>> {
    let streamed = serde_json::Deserializer::from_slice(bytes)
        .into_iter::<serde_json::Value>()
        .take(MAX_DOCUMENTS + 1)
        .collect::<Result<Vec<_>, _>>();
    match streamed {
        Ok(values) => values.into_iter().map(Some).collect(),
        Err(_) => bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
            .take(MAX_DOCUMENTS + 1)
            .map(|line| serde_json::from_slice(line).ok())
            .collect(),
    }
}

fn ranked(results: Results, query: &str) -> Vec<SearchEntry> {
    let wanted = words(query);
    let lead = results
        .videos
        .iter()
        .find(|video| !wanted.is_empty() && wanted.is_subset(&words(video.title())))
        .map(|video| video.id().to_owned());
    let order = lead
        .into_iter()
        .chain(results.songs.iter().map(|song| song.id().to_owned()))
        .chain(results.videos.iter().map(|video| video.id().to_owned()))
        .collect::<Vec<_>>();
    let mut songs = first_by_id(results.songs);
    let mut videos = first_by_id(results.videos);
    let mut emitted = HashSet::new();
    order
        .into_iter()
        .filter(|id| emitted.insert(id.clone()))
        .filter_map(|id| match (songs.remove(&id), videos.remove(&id)) {
            (Some(song), Some(video)) => Some(song.enriched(video)),
            (song, video) => song.or(video),
        })
        .collect()
}

fn first_by_id(entries: Vec<SearchEntry>) -> HashMap<String, SearchEntry> {
    let mut indexed = HashMap::with_capacity(entries.len());
    for entry in entries {
        indexed.entry(entry.id().to_owned()).or_insert(entry);
    }
    indexed
}

fn words(text: &str) -> HashSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}
