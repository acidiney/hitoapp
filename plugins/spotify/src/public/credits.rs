use hito_plugin_sdk::{CreditRole, CreditedArtist, bracketed_feature_names};

pub(super) struct Credited {
    pub name: String,
    pub id: Option<String>,
}

/// The credited artists in Spotify's order. An artist is featured only when the title's own
/// feature clause names it; the credit always keeps at least one main artist.
pub(super) fn credited_artists(artists: &[Credited], title: &str) -> Vec<CreditedArtist> {
    let named = bracketed_feature_names(title);
    let featured: Vec<bool> = artists
        .iter()
        .map(|artist| named.contains(&artist.name.trim().to_lowercase()))
        .collect();
    let all_featured = featured.iter().all(|featured| *featured);
    artists
        .iter()
        .enumerate()
        .map(|(index, artist)| CreditedArtist {
            name: artist.name.clone(),
            provider_artist_id: artist.id.clone(),
            page_url: artist
                .id
                .as_ref()
                .map(|id| format!("https://open.spotify.com/artist/{id}")),
            role: if featured[index] && !all_featured {
                CreditRole::Featured
            } else {
                CreditRole::Main
            },
            join: (index + 1 < artists.len()).then(|| ", ".to_owned()),
        })
        .collect()
}
