use hito_plugin_sdk::ProviderRecording;
use serde::Serialize;
use std::collections::BTreeSet;

pub(super) const POLICY: &str = "hito.musical-source.v1";

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
        self.identity_matches(candidate)
            && (!known_artist(&candidate.artist)
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

    pub(super) fn duration_distance(&self, candidate: &ProviderRecording) -> u64 {
        candidate
            .duration_ms
            .map(|duration| duration.abs_diff(self.duration_ms.max(0) as u64))
            .unwrap_or(u64::MAX)
    }

    fn identity_matches(&self, candidate: &ProviderRecording) -> bool {
        let title = normalized(&self.title);
        !title.is_empty()
            && title == normalized(&candidate.title)
            && versions(&self.title, self.album.as_deref())
                == versions(
                    &candidate.title,
                    candidate
                        .release
                        .as_ref()
                        .map(|release| release.title.as_str()),
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

fn known_artist(value: &str) -> bool {
    let artist = normalized(value);
    !artist.is_empty()
        && !value.trim().to_lowercase().starts_with("channel:")
        && !matches!(
            artist.as_str(),
            "unknown" | "unknown artist" | "various artists"
        )
}

fn versions(title: &str, album: Option<&str>) -> BTreeSet<&'static str> {
    let mut result = BTreeSet::new();
    for value in [Some(title), album].into_iter().flatten() {
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
            || normalized.starts_with("live from ")
            || normalized.starts_with("live in ")
            || normalized.ends_with(" live")
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
