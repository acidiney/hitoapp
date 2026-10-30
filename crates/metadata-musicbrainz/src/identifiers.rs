use crate::Error;
use serde::Deserialize;
use std::collections::HashSet;
use uuid::Uuid;

const MAX_RECORDINGS: usize = 100;
const MAX_CREDIT_MEMBERS: usize = 64;

/// A MusicBrainz recording that carries a looked-up ISRC, with the distinct artists in its credit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsrcRecording {
    pub id: String,
    pub artist_ids: Vec<String>,
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
