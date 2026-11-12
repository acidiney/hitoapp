use hito_plugin_sdk::{ProviderRecording, RecordingMetadata, SearchRequest};
use std::collections::BTreeSet;

#[derive(PartialEq, Eq, sqlx::FromRow)]
pub(super) struct AutomaticMetadata {
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub album: Option<String>,
}

impl AutomaticMetadata {
    pub(super) fn query(&self) -> Option<SearchRequest> {
        if normalized(&self.title).is_empty()
            || matches!(
                normalized(&self.artist).as_str(),
                "" | "unknown" | "unknown artist" | "various artists"
            )
            || self.duration_ms < 1000
        {
            return None;
        }
        let request = SearchRequest {
            query: format!("{} {}", self.artist.trim(), self.title.trim()),
            cursor: None,
            limit: 20,
        };
        request.validate().is_ok().then_some(request)
    }

    pub(super) fn matches(&self, candidate: &ProviderRecording) -> bool {
        normalized(&self.title) == normalized(&candidate.title)
            && normalized(&self.artist) == normalized(&candidate.artist)
            && candidate.duration_ms.is_some_and(|duration| {
                let target = self.duration_ms as u64;
                let tolerance = 2000.min(target.min(duration) / 20);
                self.duration_ms >= 1000
                    && duration >= 1000
                    && duration.abs_diff(target) <= tolerance
            })
            && candidate.release.as_ref().is_none_or(|release| {
                album_versions([self.album.as_deref()])
                    == album_versions([Some(release.title.as_str())])
            })
    }

    pub(super) fn matches_metadata(&self, metadata: &RecordingMetadata) -> bool {
        self.matches(&metadata.recording)
            && metadata
                .musical_title
                .as_deref()
                .is_none_or(|title| normalized(title) == normalized(&self.title))
            && album_versions([self.album.as_deref()])
                == album_versions([
                    metadata.album_title.as_deref(),
                    metadata
                        .recording
                        .release
                        .as_ref()
                        .map(|release| release.title.as_str()),
                ])
    }
}

fn album_versions<'a>(albums: impl IntoIterator<Item = Option<&'a str>>) -> BTreeSet<String> {
    albums
        .into_iter()
        .flatten()
        .filter_map(|album| {
            let value = normalized(album);
            let variant = value.split_whitespace().any(|word| {
                matches!(
                    word,
                    "live"
                        | "remix"
                        | "remixed"
                        | "mix"
                        | "acoustic"
                        | "unplugged"
                        | "remaster"
                        | "remastered"
                        | "demo"
                        | "edit"
                        | "radio"
                        | "extended"
                        | "instrumental"
                        | "karaoke"
                        | "cover"
                        | "slowed"
                        | "sped"
                        | "nightcore"
                        | "alternate"
                        | "alternative"
                        | "session"
                        | "rehearsal"
                        | "mono"
                        | "stereo"
                        | "clean"
                        | "explicit"
                )
            });
            variant.then_some(value)
        })
        .collect()
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
