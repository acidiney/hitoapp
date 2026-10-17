use crate::{
    error::Error,
    transport::{FORMAT, Selection, Track, Transport},
};
use hito_plugin_sdk::{
    ProviderRecording, ProviderRelease, ProviderSource, SearchPage, SearchRequest, SourcePage,
};
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
            artist: track.artist_name.clone(),
        })
    };
    let recording = ProviderRecording {
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
