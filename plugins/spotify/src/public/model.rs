use crate::error::Error;
use hito_plugin_sdk::{
    ProviderPlaylistEntry, ProviderPlaylistItem, ProviderRecording, ProviderRelease,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Response {
    data: Data,
    errors: Option<Vec<serde::de::IgnoredAny>>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(rename = "playlistV2")]
    playlist: Playlist,
}

#[derive(Deserialize)]
struct Playlist {
    #[serde(rename = "__typename")]
    kind: String,
    uri: String,
    name: String,
    content: Content,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Content {
    items: Vec<Entry>,
    total_count: usize,
    paging_info: PagingInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PagingInfo {
    next_offset: Option<usize>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "itemV2")]
    item: Item,
}

#[derive(Deserialize)]
struct Item {
    data: Option<ContentItem>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum ContentItem {
    Track(Track),
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Track {
    uri: String,
    name: String,
    artists: Artists,
    duration: Duration,
    album_of_track: Option<Album>,
    disc_number: Option<u32>,
    track_number: Option<u32>,
}

#[derive(Deserialize)]
struct Album {
    uri: String,
    name: String,
}

#[derive(Deserialize)]
struct Artists {
    items: Vec<Artist>,
}

#[derive(Deserialize)]
struct Artist {
    profile: Profile,
}

#[derive(Deserialize)]
struct Profile {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Duration {
    total_milliseconds: u64,
}

pub(super) struct Page {
    pub name: String,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<ProviderPlaylistEntry>,
}

pub(super) fn read(bytes: &[u8], id: &str) -> Result<Page, Error> {
    let response: Response = serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    let playlist = response.data.playlist;
    if response.errors.is_some_and(|errors| !errors.is_empty())
        || playlist.kind != "Playlist"
        || playlist.uri != format!("spotify:playlist:{id}")
        || playlist.name.trim().is_empty()
        || playlist.name.len() > 256
        || playlist.name.chars().any(char::is_control)
        || playlist.content.items.len() > 100
    {
        return Err(Error::InvalidResponse);
    }
    let items = playlist
        .content
        .items
        .into_iter()
        .map(|entry| match entry.item.data {
            Some(ContentItem::Track(track)) => normalize(track),
            Some(ContentItem::Unknown) | None => Ok(ProviderPlaylistEntry {
                occurrence_id: None,
                item: ProviderPlaylistItem::Unknown {},
            }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Page {
        name: playlist.name,
        total: playlist.content.total_count,
        next_offset: playlist.content.paging_info.next_offset,
        items,
    })
}

fn normalize(track: Track) -> Result<ProviderPlaylistEntry, Error> {
    let id = track
        .uri
        .strip_prefix("spotify:track:")
        .filter(|id| valid_id(id))
        .ok_or(Error::InvalidResponse)?;
    if track.artists.items.is_empty() || track.artists.items.len() > 32 {
        return Err(Error::InvalidResponse);
    }
    let artist = track
        .artists
        .items
        .into_iter()
        .map(|artist| artist.profile.name)
        .collect::<Vec<_>>()
        .join(", ");
    let release = if let Some(album) = track.album_of_track {
        let id = album
            .uri
            .strip_prefix("spotify:album:")
            .filter(|id| valid_id(id))
            .ok_or(Error::InvalidResponse)?;
        Some(ProviderRelease {
            id: id.to_owned(),
            title: album.name,
            artist: None,
        })
    } else {
        None
    };
    let recording = ProviderRecording {
        isrc: None,
        id: id.to_owned(),
        title: track.name,
        artist,
        duration_ms: Some(track.duration.total_milliseconds),
        release,
        disc_number: track.disc_number,
        track_number: track.track_number,
        revision: None,
        attribution: None,
    };
    recording.validate().map_err(|_| Error::InvalidResponse)?;
    Ok(ProviderPlaylistEntry {
        occurrence_id: None,
        item: ProviderPlaylistItem::Recording { recording },
    })
}

pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}
