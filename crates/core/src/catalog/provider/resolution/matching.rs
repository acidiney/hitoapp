use hito_plugin_sdk::{ProviderRecording, RecordingMetadata};
use serde::Serialize;
use std::collections::BTreeSet;

pub(super) const POLICY: &str = "hito.musical-source.v1";
pub(super) const METADATA_POLICY: &str = "hito.musical-source.metadata.v1";

#[derive(Serialize, sqlx::FromRow)]
pub(super) struct MusicalRecording {
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub album: Option<String>,
}

impl MusicalRecording {
    pub(super) fn query(&self) -> Option<String> {
        if !known_artist(&self.artist) || self.duration_ms < 1000 {
            return None;
        }
        let query = format!("{} {}", self.artist.trim(), self.title.trim());
        (query.len() <= 512 && !query.chars().any(char::is_control)).then_some(query)
    }

    pub(super) fn plausible(&self, candidate: &ProviderRecording) -> bool {
        self.identity_matches(candidate) && self.plausible_metadata_summary(candidate)
    }

    pub(super) fn plausible_metadata_summary(&self, candidate: &ProviderRecording) -> bool {
        (!known_artist(&candidate.artist)
            || normalized(&self.artist) == normalized(&candidate.artist))
            && candidate
                .duration_ms
                .is_none_or(|duration| self.duration_matches(duration))
    }

    pub(super) fn accepts(&self, candidate: &ProviderRecording) -> bool {
        self.identity_matches(candidate)
            && known_artist(&self.artist)
            && known_artist(&candidate.artist)
            && normalized(&self.artist) == normalized(&candidate.artist)
            && candidate
                .duration_ms
                .is_some_and(|duration| self.duration_matches(duration))
    }

    pub(super) fn metadata_rejection(&self, metadata: &RecordingMetadata) -> Option<&'static str> {
        let Some(musical_title) = metadata.musical_title.as_deref() else {
            return Some("missing_musical_title");
        };
        let candidate = &metadata.recording;
        let musical_title = without_featured_artists(musical_title);
        let title = normalized(&without_featured_artists(&self.title));
        if title.is_empty() || title != normalized(&musical_title) {
            return Some("musical_title_mismatch");
        }
        let mut candidate_versions = versions(
            &without_featured_artists(&candidate.title),
            candidate
                .release
                .as_ref()
                .map(|release| release.title.as_str()),
            Some((&candidate.artist, &musical_title)),
        );
        candidate_versions.extend(versions(
            &musical_title,
            metadata.album_title.as_deref(),
            None,
        ));
        if versions(
            &without_featured_artists(&self.title),
            self.album.as_deref(),
            None,
        ) != candidate_versions
        {
            return Some("version_mismatch");
        }
        if !known_artist(&self.artist) {
            return Some("unknown_target_artist");
        }
        if !known_artist(&candidate.artist) {
            return Some("unknown_candidate_artist");
        }
        if normalized(&self.artist) != normalized(&candidate.artist) {
            return Some("artist_mismatch");
        }
        match candidate.duration_ms {
            None => Some("missing_duration"),
            Some(duration) if !self.duration_matches(duration) => Some("duration_mismatch"),
            Some(_) => None,
        }
    }

    fn identity_matches(&self, candidate: &ProviderRecording) -> bool {
        let target_title = without_featured_artists(&self.title);
        let candidate_title = without_featured_artists(&candidate.title);
        let title = normalized(&target_title);
        !title.is_empty()
            && title == normalized(&candidate_title)
            && versions(&target_title, self.album.as_deref(), None)
                == versions(
                    &candidate_title,
                    candidate
                        .release
                        .as_ref()
                        .map(|release| release.title.as_str()),
                    None,
                )
    }

    fn duration_matches(&self, duration: u64) -> bool {
        if self.duration_ms < 1000 || duration < 1000 {
            return false;
        }
        let target = self.duration_ms as u64;
        let tolerance = (target / 50).clamp(3000, 10000);
        duration.abs_diff(target) <= tolerance
    }
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn without_featured_artists(title: &str) -> String {
    let mut kept = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(open) = rest.find(['(', '[']) {
        let close = if rest[open..].starts_with('(') {
            ')'
        } else {
            ']'
        };
        let Some(length) = rest[open + 1..].find(close) else {
            break;
        };
        let inner = &rest[open + 1..open + 1 + length];
        kept.push_str(&rest[..open]);
        if !featured_credit(inner) {
            kept.push_str(&rest[open..open + length + 2]);
        }
        rest = &rest[open + length + 2..];
    }
    kept.push_str(rest);
    let lowered = kept.to_lowercase();
    let cut = [" feat. ", " feat ", " ft. ", " ft ", " featuring "]
        .iter()
        .filter_map(|marker| lowered.find(marker))
        .min();
    match cut {
        Some(index) if kept.is_char_boundary(index) => kept[..index].trim().to_owned(),
        _ => kept.trim().to_owned(),
    }
}

fn featured_credit(qualifier: &str) -> bool {
    let qualifier = qualifier.trim().to_lowercase();
    ["feat.", "feat ", "ft.", "ft ", "featuring "]
        .iter()
        .any(|marker| qualifier.starts_with(marker))
}

fn known_artist(value: &str) -> bool {
    let artist = normalized(value);
    !artist.is_empty()
        && !value.trim().to_lowercase().starts_with("channel:")
        && !matches!(
            artist.as_str(),
            "unknown" | "unknown artist" | "various artists"
        )
}

fn versions(
    title: &str,
    album: Option<&str>,
    canonical: Option<(&str, &str)>,
) -> BTreeSet<&'static str> {
    let mut result = BTreeSet::new();
    let canonical_wrapper = canonical.is_some_and(|(artist, musical_title)| {
        known_artist(artist)
            && normalized(title) == format!("{} {}", normalized(artist), normalized(musical_title))
    });
    if album.is_some_and(|album| {
        normalized(album)
            .split_whitespace()
            .any(|word| word == "live")
    }) {
        result.insert("live");
    }
    for (value, canonical_wrapper) in [(Some(title), canonical_wrapper), (album, false)] {
        let Some(value) = value else {
            continue;
        };
        let normalized = normalized(value);
        let words = normalized.split_whitespace().collect::<Vec<_>>();
        for (name, marker) in [
            ("remix", "remix"),
            ("karaoke", "karaoke"),
            ("cover", "cover"),
            ("slowed", "slowed"),
            ("instrumental", "instrumental"),
        ] {
            if words.contains(&marker) {
                result.insert(name);
            }
        }
        if normalized.contains("sped up") || words.contains(&"nightcore") {
            result.insert("sped_up");
        }
        if normalized.starts_with("live at ")
            || normalized.contains(" live at ")
            || normalized.starts_with("live from ")
            || normalized.contains(" live from ")
            || normalized.starts_with("live in ")
            || normalized.contains(" live in ")
            || (!canonical_wrapper && normalized.ends_with(" live"))
            || normalized.contains(" live version")
            || qualified_live(value)
        {
            result.insert("live");
        }
    }
    result
}

fn qualified_live(value: &str) -> bool {
    value.split(['(', '[']).skip(1).any(|part| {
        let qualifier = normalized(part.split([')', ']']).next().unwrap_or_default());
        qualifier == "live" || qualifier.starts_with("live ")
    })
}
