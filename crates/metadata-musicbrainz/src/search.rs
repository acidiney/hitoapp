use crate::{Error, projection};
use hito_plugin_sdk::{
    ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest, CatalogArtistRef,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CURSOR_VERSION: &str = "mb-artist-search-v1";

#[derive(Deserialize)]
struct Search {
    count: Option<u32>,
    offset: u32,
    artists: Vec<Artist>,
}

#[derive(Deserialize)]
struct Artist {
    id: String,
    name: String,
    #[serde(default)]
    score: Option<Score>,
    #[serde(rename = "type")]
    kind: Option<String>,
    disambiguation: Option<String>,
    country: Option<String>,
    #[serde(rename = "life-span")]
    life_span: Option<LifeSpan>,
}

#[derive(Deserialize)]
struct LifeSpan {
    begin: Option<String>,
}

impl LifeSpan {
    fn year(&self) -> Option<u16> {
        let begin = self.begin.as_deref()?;
        let year = begin.get(..4)?;
        if !year.bytes().all(|byte| byte.is_ascii_digit())
            || begin.len() > 4 && begin.as_bytes()[4] != b'-'
        {
            return None;
        }
        year.parse()
            .ok()
            .filter(|year| (1000..=2999).contains(year))
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Score {
    Number(u32),
    Text(String),
}

impl Score {
    fn value(&self) -> Option<u32> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(text) => text.parse().ok(),
        }
    }
}

pub const NAME_SEARCH_LIMIT: u16 = 100;
const EXACT_SCORE: u32 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameCandidate {
    pub id: String,
    pub name: String,
    pub score: u32,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub country: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameSearch {
    pub total: u32,
    pub items: Vec<NameCandidate>,
}

impl NameSearch {
    pub fn conclusive(&self) -> bool {
        self.items.len() as u32 >= self.total
            || self
                .items
                .last()
                .is_some_and(|last| last.score < EXACT_SCORE)
    }
}

pub(super) fn name_search(bytes: &[u8]) -> Result<NameSearch, Error> {
    let search: Search = serde_json::from_slice(bytes).map_err(|_| Error::InvalidSearch)?;
    let total = search.count.ok_or(Error::InvalidSearch)?;
    if search.offset != 0
        || search.artists.len() > usize::from(NAME_SEARCH_LIMIT)
        || search.artists.len() as u32 > total
    {
        return Err(Error::InvalidSearch);
    }
    let mut previous = u32::MAX;
    let items = search
        .artists
        .into_iter()
        .map(|artist| {
            let score = artist
                .score
                .as_ref()
                .and_then(Score::value)
                .ok_or(Error::InvalidSearch)?;
            if score > previous || score > EXACT_SCORE {
                return Err(Error::InvalidSearch);
            }
            previous = score;
            let candidate = artist.into_candidate()?;
            let country = candidate.country.clone();
            Ok(NameCandidate {
                id: candidate.artist.id,
                name: candidate.artist.name,
                score,
                kind: candidate
                    .kind
                    .and_then(|kind| serde_json::to_value(kind).ok())
                    .and_then(|kind| kind.as_str().map(str::to_owned)),
                disambiguation: candidate.disambiguation,
                country,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(NameSearch { total, items })
}

pub(super) fn request_offset(request: &ArtistSearchRequest) -> Result<u32, Error> {
    request
        .validate()
        .map_err(|_| Error::InvalidSearchRequest)?;
    if request.snapshot.is_some() {
        return Err(Error::InvalidSearchRequest);
    }
    let Some(cursor) = &request.cursor else {
        return Ok(0);
    };
    let fingerprint = fingerprint(request);
    let mut fields = cursor.split(':');
    if fields.next() != Some(CURSOR_VERSION) || fields.next() != Some(fingerprint.as_str()) {
        return Err(Error::InvalidSearchRequest);
    }
    let offset = fields.next().ok_or(Error::InvalidSearchRequest)?;
    let parsed = offset
        .parse::<u32>()
        .map_err(|_| Error::InvalidSearchRequest)?;
    if fields.next().is_some() || parsed == 0 || parsed.to_string() != offset {
        return Err(Error::InvalidSearchRequest);
    }
    Ok(parsed)
}

pub(super) fn literal_query(query: &str) -> String {
    let mut literal = String::with_capacity(query.len() * 2 + 2);
    literal.push('"');
    for character in query.chars() {
        if matches!(
            character,
            '+' | '-'
                | '&'
                | '|'
                | '!'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '^'
                | '"'
                | '~'
                | '*'
                | '?'
                | ':'
                | '\\'
                | '/'
        ) {
            literal.push('\\');
        }
        literal.push(character);
    }
    literal.push('"');
    literal
}

pub(super) fn page(
    bytes: &[u8],
    request: &ArtistSearchRequest,
    offset: u32,
) -> Result<ArtistSearchPage, Error> {
    let search: Search = serde_json::from_slice(bytes).map_err(|_| Error::InvalidSearch)?;
    if search.offset != offset || search.artists.len() > usize::from(request.limit) {
        return Err(Error::InvalidSearch);
    }
    let next_offset = offset
        .checked_add(search.artists.len() as u32)
        .ok_or(Error::InvalidSearch)?;
    if search.count.is_some_and(|count| count < next_offset) {
        return Err(Error::InvalidSearch);
    }
    let more = search.count.is_none_or(|count| next_offset < count);
    if more && search.artists.is_empty() && search.count.is_some() {
        return Err(Error::InvalidSearch);
    }
    let next_cursor = (more && !search.artists.is_empty())
        .then(|| format!("{CURSOR_VERSION}:{}:{next_offset}", fingerprint(request)));
    let items = search
        .artists
        .into_iter()
        .map(Artist::into_candidate)
        .collect::<Result<Vec<_>, _>>()?;
    let page = ArtistSearchPage {
        query: request.query.clone(),
        snapshot: None,
        items,
        total_count: search.count,
        next_cursor,
    };
    page.validate_for(request)
        .map_err(|_| Error::InvalidSearch)?;
    Ok(page)
}

impl Artist {
    fn into_candidate(self) -> Result<ArtistSearchCandidate, Error> {
        let id = Uuid::parse_str(&self.id).map_err(|_| Error::InvalidSearch)?;
        if id.is_nil() || id.hyphenated().to_string() != self.id {
            return Err(Error::InvalidSearch);
        }
        let candidate = ArtistSearchCandidate {
            artist: CatalogArtistRef {
                id: self.id,
                name: self.name,
            },
            kind: self.kind.as_deref().and_then(projection::artist_kind),
            disambiguation: self.disambiguation.filter(|value| !value.is_empty()),
            country: self
                .country
                .filter(|value| !value.is_empty() && value.len() <= 8),
            begin_year: self.life_span.as_ref().and_then(LifeSpan::year),
        };
        candidate.validate().map_err(|_| Error::InvalidSearch)?;
        Ok(candidate)
    }
}

fn fingerprint(request: &ArtistSearchRequest) -> String {
    let mut digest = Sha256::new();
    digest.update(b"hito-musicbrainz-artist-search-v1\0");
    digest.update(request.limit.to_be_bytes());
    digest.update(request.query.as_bytes());
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn a_name_search_keeps_the_kind_disambiguation_and_country_of_each_artist() {
        let page = br#"{"count":2,"offset":0,"artists":[
            {"id":"11111111-1111-4111-8111-111111111111","name":"Nova","score":100,"type":"Group",
             "disambiguation":"Lisbon fado band","country":"PT"},
            {"id":"22222222-2222-4222-8222-222222222222","name":"Nova","score":99,"type":"Unknown kind",
             "disambiguation":"","country":""}]}"#;
        let found = name_search(page).unwrap();
        assert_eq!(found.items[0].id, ID);
        assert_eq!(found.items[0].kind.as_deref(), Some("group"));
        assert_eq!(
            found.items[0].disambiguation.as_deref(),
            Some("Lisbon fado band")
        );
        assert_eq!(found.items[0].country.as_deref(), Some("PT"));
        assert_eq!(found.items[1].kind, None, "An unknown type is not stated");
        assert_eq!(found.items[1].disambiguation, None);
        assert_eq!(
            found.items[1].country, None,
            "An empty country is not stated"
        );
    }

    #[test]
    fn a_candidate_states_its_country_and_the_year_its_life_span_began() {
        let page = br#"{"count":4,"offset":0,"artists":[
            {"id":"11111111-1111-4111-8111-111111111111","name":"A","score":100,
             "country":"GB","life-span":{"begin":"2008-04-20"}},
            {"id":"22222222-2222-4222-8222-222222222222","name":"B","score":99,
             "life-span":{"begin":"1971"}},
            {"id":"33333333-3333-4333-8333-333333333333","name":"C","score":98,
             "country":"","life-span":{"begin":"20x8"}},
            {"id":"44444444-4444-4444-8444-444444444444","name":"D","score":97,
             "life-span":{"begin":null}}]}"#;
        let found = name_search(page).unwrap();
        assert_eq!(found.items[0].country.as_deref(), Some("GB"));
        let request = ArtistSearchRequest {
            query: "x".into(),
            cursor: None,
            snapshot: None,
            limit: 20,
        };
        let candidates = super::page(page, &request, 0).unwrap().items;
        let started: Vec<Option<u16>> = candidates.iter().map(|item| item.begin_year).collect();
        assert_eq!(started, vec![Some(2008), Some(1971), None, None]);
        assert_eq!(candidates[2].country, None);
    }
}
