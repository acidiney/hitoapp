use crate::Error;
use hito_plugin_sdk::{
    CatalogArtistRef, CatalogCreditMember, CatalogEditionMedium, CatalogEditionTrackList,
    CatalogTrackKind, CatalogTrackOccurrence, EditionTrackListRequest, MAX_CATALOG_EDITION_MEDIA,
    MAX_CATALOG_EDITION_TRACKS,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
struct Edition {
    id: String,
    title: String,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<Credit>>,
    #[serde(rename = "release-group")]
    release_group: ReleaseGroup,
    media: Vec<Medium>,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    id: String,
}

#[derive(Deserialize)]
struct Medium {
    position: u32,
    title: Option<String>,
    format: Option<String>,
    #[serde(rename = "track-count")]
    track_count: u32,
    #[serde(rename = "track-offset")]
    track_offset: Option<u32>,
    pregap: Option<Track>,
    #[serde(default)]
    tracks: Vec<Track>,
    #[serde(rename = "data-tracks", default)]
    data_tracks: Vec<Track>,
}

#[derive(Deserialize)]
struct Track {
    id: String,
    position: u32,
    number: String,
    title: String,
    length: Option<u64>,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<Credit>>,
    recording: Option<Recording>,
}

#[derive(Deserialize)]
struct Recording {
    id: String,
}

#[derive(Deserialize)]
pub(super) struct Credit {
    artist: Option<CreditArtist>,
    name: String,
    joinphrase: String,
}

#[derive(Deserialize)]
struct CreditArtist {
    id: String,
    name: String,
}

pub(super) fn validate_request(request: &EditionTrackListRequest) -> Result<(), Error> {
    request
        .validate()
        .map_err(|_| Error::InvalidEditionRequest)?;
    if !canonical_mbid(&request.edition_id) || !canonical_mbid(&request.release_group_id) {
        return Err(Error::InvalidEditionRequest);
    }
    Ok(())
}

pub(super) fn track_list(
    bytes: &[u8],
    request: &EditionTrackListRequest,
) -> Result<CatalogEditionTrackList, Error> {
    let edition: Edition = serde_json::from_slice(bytes).map_err(|_| Error::InvalidEdition)?;
    if edition.id != request.edition_id
        || edition.release_group.id != request.release_group_id
        || edition.media.len() > MAX_CATALOG_EDITION_MEDIA
    {
        return Err(Error::InvalidEdition);
    }
    let mut total_tracks = 0;
    for medium in &edition.media {
        total_tracks +=
            medium.tracks.len() + medium.data_tracks.len() + usize::from(medium.pregap.is_some());
        if total_tracks > MAX_CATALOG_EDITION_TRACKS {
            return Err(Error::InvalidEdition);
        }
    }
    let mut media = edition
        .media
        .into_iter()
        .map(Medium::into_catalog)
        .collect::<Result<Vec<_>, _>>()?;
    media.sort_by_key(|medium| medium.position);
    if media
        .iter()
        .enumerate()
        .any(|(index, medium)| medium.position as usize != index + 1)
    {
        return Err(Error::InvalidEdition);
    }
    let catalog = CatalogEditionTrackList {
        id: edition.id,
        release_group_id: edition.release_group.id,
        title: edition.title,
        artist_credit: credit(edition.artist_credit)?,
        revision: None,
        media,
    };
    catalog
        .validate_for(request)
        .map_err(|_| Error::InvalidEdition)?;
    Ok(catalog)
}

impl Medium {
    fn into_catalog(self) -> Result<CatalogEditionMedium, Error> {
        if self.tracks.len() != self.track_count as usize {
            return Err(Error::InvalidEdition);
        }
        let expected_offset = (!self.tracks.is_empty()).then_some(0);
        if self.track_offset != expected_offset {
            return Err(Error::InvalidEdition);
        }
        let has_pregap = self.pregap.is_some();
        let mut tracks = Vec::with_capacity(
            self.tracks.len() + self.data_tracks.len() + usize::from(has_pregap),
        );
        if let Some(pregap) = self.pregap {
            tracks.push(pregap.into_catalog(CatalogTrackKind::Pregap)?);
        }
        for track in self.tracks {
            tracks.push(track.into_catalog(CatalogTrackKind::Audio)?);
        }
        for track in self.data_tracks {
            tracks.push(track.into_catalog(CatalogTrackKind::Data)?);
        }
        tracks.sort_by_key(|track| track.position);
        let first_position = usize::from(!has_pregap);
        if tracks
            .iter()
            .enumerate()
            .any(|(index, track)| track.position as usize != index + first_position)
        {
            return Err(Error::InvalidEdition);
        }
        Ok(CatalogEditionMedium {
            position: self.position,
            title: self.title.filter(|value| !value.is_empty()),
            format: self.format.filter(|value| !value.is_empty()),
            tracks,
        })
    }
}

impl Track {
    fn into_catalog(self, kind: CatalogTrackKind) -> Result<CatalogTrackOccurrence, Error> {
        if !canonical_mbid(&self.id) {
            return Err(Error::InvalidEdition);
        }
        let recording_id = self
            .recording
            .map(|recording| {
                if !canonical_mbid(&recording.id) {
                    return Err(Error::InvalidEdition);
                }
                Ok(recording.id)
            })
            .transpose()?;
        Ok(CatalogTrackOccurrence {
            id: self.id,
            position: self.position,
            number: (!self.number.is_empty()).then_some(self.number),
            kind,
            title: self.title,
            artist_credit: credit(self.artist_credit)?,
            duration_ms: self.length,
            recording_id,
        })
    }
}

pub(super) fn credit(
    members: Option<Vec<Credit>>,
) -> Result<Option<Vec<CatalogCreditMember>>, Error> {
    members
        .map(|members| {
            members
                .into_iter()
                .map(|member| {
                    let artist = member
                        .artist
                        .map(|artist| {
                            if !canonical_mbid(&artist.id) {
                                return Err(Error::InvalidEdition);
                            }
                            Ok(CatalogArtistRef {
                                id: artist.id,
                                name: artist.name,
                            })
                        })
                        .transpose()?;
                    Ok(CatalogCreditMember {
                        artist,
                        credited_name: member.name,
                        join_phrase: member.joinphrase,
                    })
                })
                .collect()
        })
        .transpose()
}

pub(super) fn canonical_mbid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}
