use crate::Error;
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, CatalogArtistRef, CatalogCreditMember,
    CatalogReleaseGroup, CatalogReleaseGroupKind, CatalogReleaseTypeFilter,
};
use serde::Deserialize;
use uuid::Uuid;

const CURSOR_VERSION: &str = "mb-release-groups-v1";
const TYPED_CURSOR_VERSION: &str = "mb-release-groups-typed-v1";

#[derive(Deserialize)]
struct Browse {
    #[serde(rename = "release-group-count")]
    count: Option<u32>,
    #[serde(rename = "release-group-offset")]
    offset: u32,
    #[serde(rename = "release-groups")]
    groups: Vec<ReleaseGroup>,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    id: String,
    title: String,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<Credit>>,
    #[serde(rename = "primary-type")]
    kind: Option<String>,
    #[serde(rename = "secondary-types")]
    secondary_types: Vec<String>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
}

#[derive(Deserialize)]
struct Credit {
    artist: Option<CreditArtist>,
    name: String,
    joinphrase: String,
}

#[derive(Deserialize)]
struct CreditArtist {
    id: String,
    name: String,
}

pub(super) fn request_offset(request: &ArtistDiscographyRequest) -> Result<u32, Error> {
    request
        .validate()
        .map_err(|_| Error::InvalidDiscographyRequest)?;
    if !canonical_mbid(&request.artist_id) || request.snapshot.is_some() {
        return Err(Error::InvalidDiscographyRequest);
    }
    let Some(cursor) = &request.cursor else {
        return Ok(0);
    };
    let mut fields = cursor.split(':');
    let version = match request.release_type {
        Some(_) => TYPED_CURSOR_VERSION,
        None => CURSOR_VERSION,
    };
    if fields.next() != Some(version) || fields.next() != Some(request.artist_id.as_str()) {
        return Err(Error::InvalidDiscographyRequest);
    }
    if let Some(filter) = request.release_type
        && fields.next() != Some(type_parameter(filter))
    {
        return Err(Error::InvalidDiscographyRequest);
    }
    if fields.next() != Some(request.limit.to_string().as_str()) {
        return Err(Error::InvalidDiscographyRequest);
    }
    let offset = fields.next().ok_or(Error::InvalidDiscographyRequest)?;
    let parsed = offset
        .parse::<u32>()
        .map_err(|_| Error::InvalidDiscographyRequest)?;
    if fields.next().is_some() || parsed == 0 || parsed.to_string() != offset {
        return Err(Error::InvalidDiscographyRequest);
    }
    Ok(parsed)
}

pub(super) fn type_parameter(filter: CatalogReleaseTypeFilter) -> &'static str {
    match filter {
        CatalogReleaseTypeFilter::Album => "album",
        CatalogReleaseTypeFilter::Ep => "ep",
        CatalogReleaseTypeFilter::Single => "single",
        CatalogReleaseTypeFilter::Other => "other",
    }
}

fn next_cursor(request: &ArtistDiscographyRequest, next_offset: u32) -> String {
    match request.release_type {
        Some(filter) => format!(
            "{TYPED_CURSOR_VERSION}:{}:{}:{}:{next_offset}",
            request.artist_id,
            type_parameter(filter),
            request.limit
        ),
        None => format!(
            "{CURSOR_VERSION}:{}:{}:{next_offset}",
            request.artist_id, request.limit
        ),
    }
}

pub(super) fn page(
    bytes: &[u8],
    request: &ArtistDiscographyRequest,
    offset: u32,
) -> Result<ArtistDiscographyPage, Error> {
    let browse: Browse = serde_json::from_slice(bytes).map_err(|_| Error::InvalidDiscography)?;
    if browse.offset != offset || browse.groups.len() > usize::from(request.limit) {
        return Err(Error::InvalidDiscography);
    }
    let next_offset = offset
        .checked_add(browse.groups.len() as u32)
        .ok_or(Error::InvalidDiscography)?;
    let more = browse.count.is_none_or(|count| next_offset < count);
    if more && browse.groups.is_empty() && browse.count.is_some() {
        return Err(Error::InvalidDiscography);
    }
    let next_cursor =
        (more && !browse.groups.is_empty()).then(|| next_cursor(request, next_offset));
    let items = browse
        .groups
        .into_iter()
        .map(ReleaseGroup::into_catalog)
        .collect::<Result<Vec<_>, _>>()?;
    let page = ArtistDiscographyPage {
        artist_id: request.artist_id.clone(),
        snapshot: None,
        items,
        total_count: browse.count,
        next_cursor,
    };
    page.validate_for(request)
        .map_err(|_| Error::InvalidDiscography)?;
    Ok(page)
}

impl ReleaseGroup {
    fn into_catalog(self) -> Result<CatalogReleaseGroup, Error> {
        if !canonical_mbid(&self.id) {
            return Err(Error::InvalidDiscography);
        }
        let artist_credit = self
            .artist_credit
            .map(|members| {
                members
                    .into_iter()
                    .map(Credit::into_catalog)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        Ok(CatalogReleaseGroup {
            id: self.id,
            title: self.title,
            artist_credit,
            kind: self.kind.as_deref().and_then(group_kind),
            secondary_types: self.secondary_types,
            first_release_date: self.first_release_date.filter(|date| !date.is_empty()),
            revision: None,
        })
    }
}

impl Credit {
    fn into_catalog(self) -> Result<CatalogCreditMember, Error> {
        let artist = self
            .artist
            .map(|artist| {
                if !canonical_mbid(&artist.id) {
                    return Err(Error::InvalidDiscography);
                }
                Ok(CatalogArtistRef {
                    id: artist.id,
                    name: artist.name,
                })
            })
            .transpose()?;
        Ok(CatalogCreditMember {
            artist,
            credited_name: self.name,
            join_phrase: self.joinphrase,
        })
    }
}

fn group_kind(value: &str) -> Option<CatalogReleaseGroupKind> {
    [
        ("Album", CatalogReleaseGroupKind::Album),
        ("EP", CatalogReleaseGroupKind::Ep),
        ("Single", CatalogReleaseGroupKind::Single),
        ("Broadcast", CatalogReleaseGroupKind::Broadcast),
        ("Other", CatalogReleaseGroupKind::Other),
    ]
    .into_iter()
    .find_map(|(name, kind)| value.eq_ignore_ascii_case(name).then_some(kind))
}

fn canonical_mbid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}
