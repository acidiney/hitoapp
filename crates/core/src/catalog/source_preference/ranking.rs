use super::{SourcePreferenceEntry, SourcePreferences};
use crate::Source;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin<'a> {
    LocalFiles,
    SavedCopies,
    Podcast,
    Provider(&'a str),
}

#[derive(Clone, Debug)]
pub(crate) struct SourceRanking {
    fallback: bool,
    entries: Vec<SourcePreferenceEntry>,
    explicit: bool,
}

impl SourceRanking {
    pub(super) fn new(preferences: SourcePreferences) -> Self {
        Self {
            explicit: preferences.revision > 0,
            fallback: preferences.fallback,
            entries: preferences.entries,
        }
    }

    pub(crate) fn fallback(&self) -> bool {
        self.fallback
    }

    pub(crate) fn position(&self, origin: Origin<'_>) -> usize {
        let local = || self.index(&SourcePreferenceEntry::LocalFiles);
        let saved = || self.index(&SourcePreferenceEntry::SavedCopies);
        match origin {
            Origin::LocalFiles => local().map_or(0, |index| index * 2),
            Origin::SavedCopies => saved().map_or(2, |index| index * 2),
            Origin::Podcast => saved().map_or(3, |index| index * 2 + 1),
            Origin::Provider(installation_id) => {
                let entry = SourcePreferenceEntry::Provider {
                    installation_id: installation_id.to_owned(),
                };
                if self.explicit {
                    self.index(&entry).map_or(usize::MAX, |index| index * 2)
                } else {
                    usize::MAX
                }
            }
        }
    }

    pub(crate) fn sort_sources(
        &self,
        sources: &mut [Source],
        installations: &HashMap<String, String>,
    ) {
        sources.sort_by_key(|source| {
            let origin = match source.provider.as_str() {
                "local" => Origin::LocalFiles,
                "download" => Origin::SavedCopies,
                "podcast" => Origin::Podcast,
                _ => installations
                    .get(&source.id)
                    .map_or(Origin::Provider(""), |id| Origin::Provider(id)),
            };
            self.position(origin)
        });
    }

    fn index(&self, entry: &SourcePreferenceEntry) -> Option<usize> {
        self.entries.iter().position(|candidate| candidate == entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, provider: &str) -> Source {
        Source {
            id: id.into(),
            recording_id: "recording".into(),
            provider: provider.into(),
            label: id.into(),
            device_id: None,
            ready: true,
            can_prepare: false,
        }
    }

    fn provider(id: &str) -> SourcePreferenceEntry {
        SourcePreferenceEntry::Provider {
            installation_id: id.into(),
        }
    }

    fn ranking(revision: i64, entries: Vec<SourcePreferenceEntry>) -> SourceRanking {
        SourceRanking::new(SourcePreferences {
            revision,
            fallback: true,
            entries,
        })
    }

    fn sorted(ranking: &SourceRanking, input: &[(&str, &str, &str)]) -> Vec<String> {
        let mut sources: Vec<Source> = input.iter().map(|row| source(row.0, row.1)).collect();
        let installations: HashMap<String, String> = input
            .iter()
            .filter(|row| row.1 == "plugin")
            .map(|row| (row.0.to_owned(), row.2.to_owned()))
            .collect();
        ranking.sort_sources(&mut sources, &installations);
        sources.into_iter().map(|source| source.id).collect()
    }

    #[test]
    fn selection_order_table() {
        let defaults = vec![
            SourcePreferenceEntry::LocalFiles,
            SourcePreferenceEntry::SavedCopies,
            provider("a"),
            provider("b"),
        ];
        let rows: Vec<(&str, SourceRanking, Vec<(&str, &str, &str)>, Vec<&str>)> = vec![
            (
                "unsaved keeps local, saved, podcast, then the incoming provider order",
                ranking(0, defaults.clone()),
                vec![
                    ("b1", "plugin", "b"),
                    ("a1", "plugin", "a"),
                    ("p", "podcast", ""),
                    ("d", "download", ""),
                    ("l", "local", ""),
                ],
                vec!["l", "d", "p", "b1", "a1"],
            ),
            (
                "a saved provider order decides between providers",
                ranking(1, defaults.clone()),
                vec![("b1", "plugin", "b"), ("a1", "plugin", "a")],
                vec!["a1", "b1"],
            ),
            (
                "the incoming community order survives inside one provider",
                ranking(1, vec![provider("b"), provider("a")]),
                vec![
                    ("a2", "plugin", "a"),
                    ("b2", "plugin", "b"),
                    ("a1", "plugin", "a"),
                    ("b1", "plugin", "b"),
                ],
                vec!["b2", "b1", "a2", "a1"],
            ),
            (
                "the user may move a provider above local files",
                ranking(
                    1,
                    vec![
                        provider("a"),
                        SourcePreferenceEntry::LocalFiles,
                        SourcePreferenceEntry::SavedCopies,
                    ],
                ),
                vec![
                    ("l", "local", ""),
                    ("d", "download", ""),
                    ("a1", "plugin", "a"),
                ],
                vec!["a1", "l", "d"],
            ),
            (
                "an unranked or unknown provider sorts after ranked ones",
                ranking(1, vec![provider("b")]),
                vec![
                    ("c1", "plugin", "c"),
                    ("x1", "plugin", "unknown"),
                    ("b1", "plugin", "b"),
                ],
                vec!["b1", "c1", "x1"],
            ),
        ];
        for (name, ranking, input, expected) in rows {
            assert_eq!(sorted(&ranking, &input), expected, "{name}");
        }
    }
}
