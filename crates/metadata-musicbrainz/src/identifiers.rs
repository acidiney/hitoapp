use crate::Error;
use serde::Deserialize;
use std::collections::HashSet;
use uuid::Uuid;

const MAX_RECORDINGS: usize = 100;
const MAX_CREDIT_MEMBERS: usize = 64;
const MAX_TITLE_BYTES: usize = 512;
const MAX_GENRES: usize = 64;
const MAX_GENRE_BYTES: usize = 64;

/// A MusicBrainz recording that carries a looked-up ISRC, with the distinct artists in its credit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsrcRecording {
    pub id: String,
    pub artist_ids: Vec<String>,
}

/// One recording credited to an artist, as listed by a browse. The length is absent when
/// MusicBrainz does not know it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistRecording {
    pub id: String,
    pub title: String,
    pub length_ms: Option<u64>,
}

/// The first page of an artist's recordings and how many MusicBrainz states in total.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistRecordings {
    pub total: u32,
    pub items: Vec<ArtistRecording>,
}

impl ArtistRecordings {
    /// Appends the next page. A page that adds nothing, or whose total changed, ends the reading.
    pub fn extend(&mut self, next: ArtistRecordings) -> bool {
        let known: HashSet<String> = self.items.iter().map(|item| item.id.clone()).collect();
        let before = self.items.len();
        self.items.extend(
            next.items
                .into_iter()
                .filter(|item| !known.contains(&item.id)),
        );
        self.items.len() > before
    }

    /// Whether the page lists every recording MusicBrainz states for the artist.
    pub fn complete(&self) -> bool {
        self.items.len() as u32 >= self.total
    }
}

/// The MusicBrainz entity kinds whose official genres can be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenreEntity {
    Artist,
    ReleaseGroup,
    Recording,
}

impl GenreEntity {
    pub(super) fn path(self) -> &'static str {
        match self {
            Self::Artist => "artist",
            Self::ReleaseGroup => "release-group",
            Self::Recording => "recording",
        }
    }
}

#[derive(Deserialize)]
struct GenreLookup {
    id: String,
    #[serde(default)]
    genres: Vec<NamedGenre>,
}

#[derive(Deserialize)]
struct NamedGenre {
    name: String,
    #[serde(default)]
    count: u32,
}

#[derive(Deserialize)]
struct RecordingBrowse {
    #[serde(rename = "recording-count")]
    count: u32,
    #[serde(rename = "recording-offset")]
    offset: u32,
    #[serde(default)]
    recordings: Vec<BrowsedRecording>,
}

#[derive(Deserialize)]
struct BrowsedRecording {
    id: String,
    title: String,
    length: Option<u64>,
}

#[derive(Deserialize)]
struct IsrcLookup {
    isrc: Option<String>,
    #[serde(default)]
    recordings: Vec<Recording>,
}

#[derive(Deserialize)]
struct Recording {
    id: String,
    #[serde(rename = "artist-credit", default)]
    credit: Vec<CreditMember>,
}

#[derive(Deserialize)]
struct CreditMember {
    artist: CreditedArtist,
}

#[derive(Deserialize)]
struct CreditedArtist {
    id: String,
}

#[derive(Deserialize)]
struct UrlLookup {
    #[serde(default)]
    relations: Vec<Relation>,
}

#[derive(Deserialize)]
struct Relation {
    #[serde(rename = "target-type")]
    target: String,
    artist: Option<CreditedArtist>,
}

pub(super) fn valid_url(resource: &str) -> bool {
    resource.len() <= 512
        && reqwest::Url::parse(resource).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.as_str() == resource
        })
        && hito_plugin_sdk::artist_page_provider(resource).is_some()
}

pub(super) fn url_artists(bytes: &[u8]) -> Result<Vec<String>, Error> {
    let lookup: UrlLookup = serde_json::from_slice(bytes).map_err(|_| Error::InvalidLookup)?;
    if lookup.relations.len() > MAX_RECORDINGS {
        return Err(Error::InvalidLookup);
    }
    let mut artists: Vec<String> = Vec::new();
    for relation in lookup.relations {
        if relation.target != "artist" {
            continue;
        }
        let artist = relation.artist.ok_or(Error::InvalidLookup)?;
        canonical_id(&artist.id)?;
        if !artists.contains(&artist.id) {
            artists.push(artist.id);
        }
    }
    Ok(artists)
}

pub(super) fn artist_recordings(bytes: &[u8], offset: u32) -> Result<ArtistRecordings, Error> {
    let browse: RecordingBrowse =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidLookup)?;
    if browse.offset != offset
        || browse.recordings.len() > MAX_RECORDINGS
        || browse.recordings.len() as u32 > browse.count
    {
        return Err(Error::InvalidLookup);
    }
    let mut seen = HashSet::new();
    let items = browse
        .recordings
        .into_iter()
        .map(|recording| {
            canonical_id(&recording.id)?;
            if !seen.insert(recording.id.clone())
                || recording.title.is_empty()
                || recording.title.len() > MAX_TITLE_BYTES
            {
                return Err(Error::InvalidLookup);
            }
            Ok(ArtistRecording {
                id: recording.id,
                title: recording.title,
                length_ms: recording.length,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ArtistRecordings {
        total: browse.count,
        items,
    })
}

/// The official genre names of a lookup, strongest first and without case-only repeats.
pub(super) fn genre_names(bytes: &[u8], id: &str) -> Result<Vec<String>, Error> {
    let lookup: GenreLookup = serde_json::from_slice(bytes).map_err(|_| Error::InvalidLookup)?;
    if lookup.id != id || lookup.genres.len() > MAX_GENRES {
        return Err(Error::InvalidLookup);
    }
    let mut genres = lookup.genres;
    if genres
        .iter()
        .any(|genre| genre.name.trim().is_empty() || genre.name.len() > MAX_GENRE_BYTES)
    {
        return Err(Error::InvalidLookup);
    }
    genres.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    let mut names: Vec<String> = Vec::new();
    for genre in genres {
        if !names
            .iter()
            .any(|known| known.to_lowercase() == genre.name.to_lowercase())
        {
            names.push(genre.name);
        }
    }
    Ok(names)
}

pub(super) fn canonical_artist(artist: &str) -> bool {
    canonical_id(artist).is_ok()
}

pub(super) fn valid_isrc(isrc: &str) -> bool {
    hito_plugin_sdk::normalize_isrc(isrc).as_deref() == Some(isrc)
}

fn canonical_id(value: &str) -> Result<(), Error> {
    match Uuid::parse_str(value) {
        Ok(id) if !id.is_nil() && id.hyphenated().to_string() == value => Ok(()),
        _ => Err(Error::InvalidLookup),
    }
}

pub(super) fn isrc_recordings(bytes: &[u8], isrc: &str) -> Result<Vec<IsrcRecording>, Error> {
    let lookup: IsrcLookup = serde_json::from_slice(bytes).map_err(|_| Error::InvalidLookup)?;
    if lookup
        .isrc
        .as_deref()
        .is_some_and(|returned| returned != isrc)
        || lookup.recordings.len() > MAX_RECORDINGS
    {
        return Err(Error::InvalidLookup);
    }
    let mut seen = HashSet::new();
    lookup
        .recordings
        .into_iter()
        .map(|recording| {
            canonical_id(&recording.id)?;
            if !seen.insert(recording.id.clone()) || recording.credit.len() > MAX_CREDIT_MEMBERS {
                return Err(Error::InvalidLookup);
            }
            let mut artist_ids: Vec<String> = Vec::new();
            for member in recording.credit {
                canonical_id(&member.artist.id)?;
                if !artist_ids.contains(&member.artist.id) {
                    artist_ids.push(member.artist.id);
                }
            }
            Ok(IsrcRecording {
                id: recording.id,
                artist_ids,
            })
        })
        .collect()
}
