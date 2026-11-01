use crate::{
    error::Error,
    transport::{Album, FORMAT, Selection, Track, Transport},
};
use hito_plugin_sdk::{
    FeaturedItem, FeaturedKind, FeaturedPage, FeaturedRequest, PlaylistAttribution,
    PlaylistDetailRequest, PlaylistItemsRequest, PlaylistPage, ProviderPlaylist,
    ProviderPlaylistEntry, ProviderPlaylistItem, ProviderRecording, ProviderRelease,
    ProviderSource, RecordingAttribution, SearchPage, SearchRequest, SourcePage,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use url::Url;

pub(crate) struct Catalog {
    transport: Arc<Transport>,
}

impl Catalog {
    pub(crate) fn new(transport: Arc<Transport>) -> Self {
        Self { transport }
    }

    pub(crate) async fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        if request.cursor.is_some() {
            return Err(Error::InvalidRequest);
        }
        let tracks = self
            .transport
            .tracks(Selection::Search {
                query: &request.query,
                limit: request.limit,
            })
            .await?;
        let items = tracks
            .iter()
            .map(recording)
            .collect::<Result<Vec<_>, _>>()?;
        let page = SearchPage {
            items,
            next_cursor: None,
        };
        page.validate(request.limit)
            .map_err(|_| Error::InvalidResponse)?;
        if serde_json::to_vec(&page)
            .map_err(|_| Error::InvalidResponse)?
            .len()
            > 262_144
        {
            return Err(Error::InvalidResponse);
        }
        Ok(page)
    }

    pub(crate) async fn detail(&self, recording_id: &str) -> Result<ProviderRecording, Error> {
        let owner = recording_owner(recording_id)?;
        recording(&self.track(owner).await?)
    }

    pub(crate) async fn sources(&self, recording_id: &str) -> Result<SourcePage, Error> {
        let owner = recording_owner(recording_id)?;
        let track = self.track(owner).await?;
        let ready = self.transport.stream_url(&track)?.is_some();
        Ok(SourcePage {
            items: vec![ProviderSource {
                id: source_id(owner),
                label: "MP3 stream".into(),
                ready,
            }],
        })
    }

    pub(crate) async fn featured(&self, request: FeaturedRequest) -> Result<FeaturedPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let albums = self.transport.featured_albums(request.limit).await?;
        let items = albums
            .iter()
            .map(|album| {
                let reference =
                    album_reference(numeric_id(&album.id).ok_or(Error::InvalidResponse)?);
                Ok(FeaturedItem {
                    id: reference.clone(),
                    title: album.name.clone(),
                    subtitle: Some(album.artist_name.clone())
                        .filter(|name| !name.trim().is_empty()),
                    kind: FeaturedKind::Album,
                    reference,
                    artwork_id: None,
                    item_count: None,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let page = FeaturedPage { items };
        page.validate_for(&request)
            .map_err(|_| Error::InvalidResponse)?;
        Ok(page)
    }

    pub(crate) async fn playlist_detail(
        &self,
        request: PlaylistDetailRequest,
    ) -> Result<ProviderPlaylist, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let owner = match &request {
            PlaylistDetailRequest::Reference { reference } => album_from_reference(reference)?,
            PlaylistDetailRequest::Canonical { playlist_id } => album_owner(playlist_id)?,
        };
        let album = self.transport.album(owner).await?;
        let detail = ProviderPlaylist {
            id: album_reference(owner),
            name: album.name.clone(),
            snapshot: snapshot(&album),
            total_count: u32::try_from(album.tracks.len()).map_err(|_| Error::InvalidResponse)?,
            attribution: PlaylistAttribution {
                label: "Jamendo".into(),
            },
        };
        detail.validate().map_err(|_| Error::InvalidResponse)?;
        Ok(detail)
    }

    pub(crate) async fn playlist_items(
        &self,
        request: PlaylistItemsRequest,
    ) -> Result<PlaylistPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let owner = album_owner(&request.playlist_id)?;
        let offset = match request.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor.parse::<usize>().map_err(|_| Error::InvalidRequest)?,
        };
        let album = self.transport.album(owner).await?;
        if snapshot(&album) != request.snapshot {
            return Err(Error::Stale);
        }
        let limit = usize::from(request.limit);
        let items = album
            .tracks
            .iter()
            .skip(offset)
            .take(limit)
            .map(|track| {
                let track = Track {
                    id: track.id.clone(),
                    name: track.name.clone(),
                    artist_name: album.artist_name.clone(),
                    duration: track.duration,
                    album_id: album.id.clone(),
                    album_name: album.name.clone(),
                    position: track.position,
                    audio: track.audio.clone(),
                };
                Ok(ProviderPlaylistEntry {
                    occurrence_id: None,
                    item: ProviderPlaylistItem::Recording {
                        recording: Box::new(recording(&track)?),
                    },
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let consumed = offset.saturating_add(items.len());
        let page = PlaylistPage {
            playlist_id: album_reference(owner),
            snapshot: request.snapshot,
            next_cursor: (consumed < album.tracks.len()).then(|| consumed.to_string()),
            items,
        };
        page.validate(request.limit)
            .map_err(|_| Error::InvalidResponse)?;
        Ok(page)
    }

    pub(crate) async fn stream(&self, source_id: &str) -> Result<Url, Error> {
        let owner = source_owner(source_id)?;
        let track = self.track(owner).await?;
        self.transport.stream_url(&track)?.ok_or(Error::NotFound)
    }

    async fn track(&self, owner: &str) -> Result<Track, Error> {
        let mut tracks = self.transport.tracks(Selection::Track(owner)).await?;
        let track = tracks.pop().ok_or(Error::NotFound)?;
        if track.id != owner {
            return Err(Error::InvalidResponse);
        }
        recording(&track)?;
        Ok(track)
    }
}

fn recording(track: &Track) -> Result<ProviderRecording, Error> {
    let owner = numeric_id(&track.id).ok_or(Error::InvalidResponse)?;
    let release = if track.album_id.is_empty() && track.album_name.is_empty() {
        None
    } else {
        let album = numeric_id(&track.album_id).ok_or(Error::InvalidResponse)?;
        Some(ProviderRelease {
            id: format!("jamendo:album:v1:{album}"),
            title: track.album_name.clone(),
            artist: Some(track.artist_name.clone()),
        })
    };
    let recording = ProviderRecording {
        isrc: None,
        artist_urls: Vec::new(),
        artists: Vec::new(),
        id: format!("jamendo:track:v1:{owner}"),
        title: track.name.clone(),
        artist: track.artist_name.clone(),
        duration_ms: Some(
            track
                .duration
                .checked_mul(1000)
                .ok_or(Error::InvalidResponse)?,
        ),
        track_number: track
            .position
            .filter(|position| *position > 0 && release.is_some()),
        release,
        disc_number: None,
        revision: None,
        attribution: Some(Box::new(RecordingAttribution {
            creators: vec![track.artist_name.clone()],
            provider_label: "Jamendo".into(),
            content_url: format!("https://www.jamendo.com/track/{owner}"),
        })),
    };
    recording.validate().map_err(|_| Error::InvalidResponse)?;
    Ok(recording)
}

fn recording_owner(value: &str) -> Result<&str, Error> {
    value
        .strip_prefix("jamendo:track:v1:")
        .and_then(numeric_id)
        .ok_or(Error::InvalidRequest)
}

fn source_owner(value: &str) -> Result<&str, Error> {
    if value.len() > 256 {
        return Err(Error::InvalidRequest);
    }
    let owner = value
        .strip_prefix("jamendo:audio:v1:")
        .and_then(|value| value.strip_suffix(":mp32"))
        .and_then(numeric_id)
        .ok_or(Error::InvalidRequest)?;
    Ok(owner)
}

fn source_id(owner: &str) -> String {
    format!("jamendo:audio:v1:{owner}:{FORMAT}")
}

fn numeric_id(value: &str) -> Option<&str> {
    if value.is_empty()
        || value.len() > 20
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || value.parse::<u64>().is_err()
    {
        return None;
    }
    Some(value)
}

fn album_reference(owner: &str) -> String {
    format!("jamendo:album:v1:{owner}")
}

fn album_owner(value: &str) -> Result<&str, Error> {
    value
        .strip_prefix("jamendo:album:v1:")
        .and_then(numeric_id)
        .ok_or(Error::InvalidRequest)
}

fn album_from_reference(reference: &str) -> Result<&str, Error> {
    if reference.starts_with("jamendo:") {
        return album_owner(reference);
    }
    let rest = reference
        .strip_prefix("https://www.jamendo.com/album/")
        .or_else(|| reference.strip_prefix("https://jamendo.com/album/"))
        .ok_or(Error::InvalidRequest)?;
    let owner = rest.split(['/', '?', '#']).next().unwrap_or_default();
    numeric_id(owner).ok_or(Error::InvalidRequest)
}

fn snapshot(album: &Album) -> String {
    let mut digest = Sha256::new();
    digest.update(album.name.as_bytes());
    for track in &album.tracks {
        digest.update([0]);
        digest.update(track.id.as_bytes());
        digest.update([0]);
        digest.update(track.name.as_bytes());
        digest.update(track.duration.to_le_bytes());
    }
    let hash = digest.finalize();
    hash.iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
