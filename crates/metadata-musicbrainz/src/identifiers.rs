use crate::Error;
use serde::Deserialize;
use std::collections::HashSet;
use uuid::Uuid;

const MAX_RECORDINGS: usize = 100;
pub const MAX_PROBES: usize = 6;
pub(super) const PROBE_RESULT_LIMIT: usize = 40;
const MAX_CREDIT_MEMBERS: usize = 64;
const MAX_TITLE_BYTES: usize = 512;
const MAX_GENRES: usize = 64;
const MAX_GENRE_BYTES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsrcRecording {
    pub id: String,
    pub artist_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistRecording {
    pub id: String,
    pub title: String,
    pub length_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingProbe {
    pub title: String,
    pub min_length_ms: u64,
    pub max_length_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistRecordings {
    pub total: u32,
    pub items: Vec<ArtistRecording>,
}

impl ArtistRecordings {
    pub fn complete(&self) -> bool {
        self.items.len() as u32 >= self.total
    }
}

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
struct RecordingSearch {
    count: u32,
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

pub(super) fn probe_query(artist: &str, probes: &[RecordingProbe]) -> Result<String, Error> {
    if !canonical_artist(artist) || probes.is_empty() || probes.len() > MAX_PROBES {
        return Err(Error::InvalidLookupRequest);
    }
    let mut clauses = Vec::with_capacity(probes.len());
    for probe in probes {
        if probe.title.trim().is_empty()
            || probe.title.len() > MAX_TITLE_BYTES
            || probe.min_length_ms > probe.max_length_ms
        {
            return Err(Error::InvalidLookupRequest);
        }
        clauses.push(format!(
            "(recording:{} AND dur:[{} TO {}])",
            crate::search::literal_query(&probe.title),
            probe.min_length_ms,
            probe.max_length_ms
        ));
    }
    Ok(format!("arid:{artist} AND ({})", clauses.join(" OR ")))
}

pub(super) fn matching_recordings(bytes: &[u8]) -> Result<ArtistRecordings, Error> {
    let search: RecordingSearch =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidLookup)?;
    if search.offset != 0
        || search.recordings.len() > PROBE_RESULT_LIMIT
        || search.recordings.len() as u32 > search.count
    {
        return Err(Error::InvalidLookup);
    }
    let mut seen = HashSet::new();
    let items = search
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
        total: search.count,
        items,
    })
}

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

pub(super) fn credited_recording(bytes: &[u8]) -> Result<IsrcRecording, Error> {
    let recording: Recording = serde_json::from_slice(bytes).map_err(|_| Error::InvalidLookup)?;
    credited(recording)
}

fn credited(recording: Recording) -> Result<IsrcRecording, Error> {
    canonical_id(&recording.id)?;
    if recording.credit.len() > MAX_CREDIT_MEMBERS {
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
}

pub(super) fn valid_recording(id: &str) -> bool {
    canonical_id(id).is_ok()
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
            if !seen.insert(recording.id.clone()) {
                return Err(Error::InvalidLookup);
            }
            credited(recording)
        })
        .collect()
}

#[derive(Deserialize)]
struct ArtistLinks {
    id: String,
    #[serde(default)]
    relations: Vec<LinkRelation>,
}

#[derive(Deserialize)]
struct LinkRelation {
    #[serde(rename = "type")]
    kind: String,
    url: Option<LinkUrl>,
}

#[derive(Deserialize)]
struct LinkUrl {
    resource: String,
}

pub(super) fn wikidata_entity(bytes: &[u8], artist: &str) -> Result<Option<String>, Error> {
    let links: ArtistLinks = serde_json::from_slice(bytes).map_err(|_| Error::InvalidLookup)?;
    if links.id != artist || links.relations.len() > 1024 {
        return Err(Error::InvalidLookup);
    }
    Ok(links
        .relations
        .into_iter()
        .filter(|relation| relation.kind == "wikidata")
        .filter_map(|relation| relation.url)
        .find_map(|url| entity_of(&url.resource)))
}

fn entity_of(resource: &str) -> Option<String> {
    let rest = resource
        .strip_prefix("https://www.wikidata.org/wiki/")
        .or_else(|| resource.strip_prefix("http://www.wikidata.org/wiki/"))
        .or_else(|| resource.strip_prefix("https://www.wikidata.org/entity/"))
        .or_else(|| resource.strip_prefix("http://www.wikidata.org/entity/"))?;
    valid_entity(rest).then(|| rest.to_owned())
}

pub(crate) fn valid_entity(entity: &str) -> bool {
    let digits = entity.strip_prefix('Q').unwrap_or_default();
    (1..=16).contains(&digits.len())
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && !digits.starts_with('0')
}
