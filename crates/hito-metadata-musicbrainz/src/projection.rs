use crate::Error;
use hito_plugin_sdk::{
    ArtistProfileRequest, CatalogArtistKind, CatalogArtistProfile, CatalogArtistRef,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Artist {
    id: String,
    name: String,
    #[serde(rename = "sort-name")]
    sort_name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    disambiguation: Option<String>,
}

pub(super) fn artist(
    bytes: &[u8],
    request: &ArtistProfileRequest,
) -> Result<CatalogArtistProfile, Error> {
    let artist: Artist = serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    let profile = CatalogArtistProfile {
        artist: CatalogArtistRef {
            id: artist.id,
            name: artist.name,
        },
        sort_name: artist.sort_name.filter(|value| !value.is_empty()),
        kind: artist.kind.as_deref().and_then(artist_kind),
        disambiguation: artist.disambiguation.filter(|value| !value.is_empty()),
        revision: None,
    };
    profile
        .validate_for(request)
        .map_err(|_| Error::InvalidResponse)?;
    Ok(profile)
}

fn artist_kind(value: &str) -> Option<CatalogArtistKind> {
    [
        ("Person", CatalogArtistKind::Person),
        ("Group", CatalogArtistKind::Group),
        ("Orchestra", CatalogArtistKind::Orchestra),
        ("Choir", CatalogArtistKind::Choir),
        ("Character", CatalogArtistKind::Character),
        ("Other", CatalogArtistKind::Other),
    ]
    .into_iter()
    .find_map(|(name, kind)| value.eq_ignore_ascii_case(name).then_some(kind))
}
