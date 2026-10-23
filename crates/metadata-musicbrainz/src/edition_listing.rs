use crate::{
    Error,
    edition::{Credit, canonical_mbid, credit},
};
use hito_plugin_sdk::{
    CatalogEditionSummary, ReleaseGroupEditionsPage, ReleaseGroupEditionsRequest,
};
use serde::Deserialize;

const CURSOR_VERSION: &str = "mb-editions-v1";

#[derive(Deserialize)]
struct Browse {
    #[serde(rename = "release-count")]
    count: Option<u32>,
    #[serde(rename = "release-offset")]
    offset: u32,
    releases: Vec<Edition>,
}

#[derive(Deserialize)]
struct Edition {
    id: String,
    title: String,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<Credit>>,
    disambiguation: Option<String>,
    date: Option<String>,
    country: Option<String>,
    #[serde(rename = "release-group")]
    release_group: ReleaseGroup,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    id: String,
}

pub(super) fn request_offset(request: &ReleaseGroupEditionsRequest) -> Result<u32, Error> {
    request
        .validate()
        .map_err(|_| Error::InvalidEditionRequest)?;
    if !canonical_mbid(&request.release_group_id) || request.snapshot.is_some() {
        return Err(Error::InvalidEditionRequest);
    }
    let Some(cursor) = &request.cursor else {
        return Ok(0);
    };
    let mut fields = cursor.split(':');
    if fields.next() != Some(CURSOR_VERSION)
        || fields.next() != Some(request.release_group_id.as_str())
        || fields.next() != Some(request.limit.to_string().as_str())
    {
        return Err(Error::InvalidEditionRequest);
    }
    let offset = fields.next().ok_or(Error::InvalidEditionRequest)?;
    let parsed = offset
        .parse::<u32>()
        .map_err(|_| Error::InvalidEditionRequest)?;
    if fields.next().is_some() || parsed == 0 || parsed.to_string() != offset {
        return Err(Error::InvalidEditionRequest);
    }
    Ok(parsed)
}

pub(super) fn page(
    bytes: &[u8],
    request: &ReleaseGroupEditionsRequest,
    offset: u32,
) -> Result<ReleaseGroupEditionsPage, Error> {
    let browse: Browse = serde_json::from_slice(bytes).map_err(|_| Error::InvalidEdition)?;
    if browse.offset != offset || browse.releases.len() > usize::from(request.limit) {
        return Err(Error::InvalidEdition);
    }
    let next_offset = offset
        .checked_add(browse.releases.len() as u32)
        .ok_or(Error::InvalidEdition)?;
    if browse.count.is_some_and(|count| next_offset > count) {
        return Err(Error::InvalidEdition);
    }
    let more = browse.count.is_none_or(|count| next_offset < count);
    if more && browse.releases.is_empty() && browse.count.is_some() {
        return Err(Error::InvalidEdition);
    }
    let next_cursor = (more && !browse.releases.is_empty()).then(|| {
        format!(
            "{CURSOR_VERSION}:{}:{}:{next_offset}",
            request.release_group_id, request.limit
        )
    });
    let items = browse
        .releases
        .into_iter()
        .map(|edition| edition.into_catalog(&request.release_group_id))
        .collect::<Result<Vec<_>, _>>()?;
    let page = ReleaseGroupEditionsPage {
        release_group_id: request.release_group_id.clone(),
        snapshot: None,
        items,
        total_count: browse.count,
        next_cursor,
    };
    page.validate_for(request)
        .map_err(|_| Error::InvalidEdition)?;
    Ok(page)
}

impl Edition {
    fn into_catalog(self, release_group_id: &str) -> Result<CatalogEditionSummary, Error> {
        if !canonical_mbid(&self.id) || self.release_group.id != release_group_id {
            return Err(Error::InvalidEdition);
        }
        Ok(CatalogEditionSummary {
            id: self.id,
            title: self.title,
            artist_credit: credit(self.artist_credit)?,
            disambiguation: self.disambiguation.filter(|value| !value.is_empty()),
            release_date: self.date.filter(|value| !value.is_empty()),
            country: self.country.filter(|value| !value.is_empty()),
            revision: None,
        })
    }
}
