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
