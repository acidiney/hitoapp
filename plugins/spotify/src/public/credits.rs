use hito_plugin_sdk::{CreditRole, CreditedArtist};
use std::collections::HashSet;

pub(super) struct Credited {
    pub name: String,
    pub id: Option<String>,
}

/// The credited artists in Spotify's order. An artist is featured only when the title's own
/// feature clause names it; the credit always keeps at least one main artist.
pub(super) fn credited_artists(artists: &[Credited], title: &str) -> Vec<CreditedArtist> {
    let named = featured_names(title);
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

const MARKERS: [&str; 5] = ["featuring ", "feat. ", "feat ", "ft. ", "ft "];

/// The byte offset in `text` where a feature marker's names begin, after optional blanks.
fn names_start(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let trimmed = lower.trim_start();
    let blanks = lower.len() - trimmed.len();
    MARKERS
        .iter()
        .find(|marker| trimmed.starts_with(*marker))
        .map(|marker| blanks + marker.len())
}

/// The lower-cased names a title's bracketed or dashed feature clauses state.
fn featured_names(title: &str) -> HashSet<String> {
    let mut clauses = Vec::new();
    for (open, close) in [('(', ')'), ('[', ']')] {
        for (start, _) in title.match_indices(open) {
            let inner = &title[start + 1..];
            if let (Some(from), Some(end)) = (names_start(inner), inner.find(close))
                && from <= end
            {
                clauses.push(&inner[from..end]);
            }
        }
    }
    for (start, _) in title.match_indices(" - ") {
        let rest = &title[start + 3..];
        if let Some(from) = names_start(rest) {
            clauses.push(&rest[from..]);
        }
    }
    let mut names = HashSet::new();
    for clause in clauses {
        let clause = clause.trim().to_lowercase();
        names.extend(
            clause
                .replace(" & ", ",")
                .replace(" and ", ",")
                .split(',')
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty()),
        );
        names.insert(clause);
    }
    names
}
