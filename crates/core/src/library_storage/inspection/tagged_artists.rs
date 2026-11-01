use hito_plugin_sdk::{CreditRole, CreditedArtist, credit_feature_names};
use lofty::{config::ParseOptions, file::TaggedFileExt, prelude::ItemKey, probe::Probe, tag::Tag};
use std::{collections::HashSet, panic::AssertUnwindSafe, path::Path};
use uuid::Uuid;

const MAX_ARTISTS: usize = 16;
const MAX_NAME_BYTES: usize = 256;

pub(in crate::library_storage::inspection) struct TaggedArtists {
    names: Vec<String>,
    ids: Vec<String>,
}

fn values(tag: &Tag, key: &ItemKey) -> Vec<String> {
    tag.get_strings(key)
        .flat_map(|value| value.split('\0'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .take(MAX_ARTISTS + 1)
        .map(|value| value.chars().take(MAX_NAME_BYTES + 1).collect())
        .collect()
}

fn guarded<T>(read: impl FnOnce() -> Option<T>) -> Option<T> {
    match std::panic::catch_unwind(AssertUnwindSafe(read)) {
        Ok(value) => value,
        Err(_) => {
            tracing::warn!("Reading tagged artists failed; the credit stays text");
            None
        }
    }
}

pub(in crate::library_storage::inspection) fn read(path: &Path) -> Option<TaggedArtists> {
    guarded(|| read_file(path))
}

fn read_file(path: &Path) -> Option<TaggedArtists> {
    let options = ParseOptions::new()
        .read_properties(false)
        .read_cover_art(false);
    let file = Probe::open(path)
        .ok()?
        .options(options)
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;
    let tags: Vec<&Tag> = file.tags().iter().collect();
    let names = tags
        .iter()
        .map(|tag| values(tag, &ItemKey::TrackArtists))
        .find(|names| !names.is_empty())
        .or_else(|| {
            tags.iter()
                .map(|tag| values(tag, &ItemKey::TrackArtist))
                .find(|names| names.len() > 1)
        })?;
    let ids = tags
        .iter()
        .map(|tag| values(tag, &ItemKey::MusicBrainzArtistId))
        .find(|ids| !ids.is_empty())
        .unwrap_or_default();
    Some(TaggedArtists { names, ids })
}

fn identifier(value: &str) -> Option<String> {
    Uuid::parse_str(value)
        .ok()
        .filter(|_| value.len() == 36)
        .map(|id| id.hyphenated().to_string())
}

pub(in crate::library_storage::inspection) fn credits(
    display: &str,
    tagged: Option<&TaggedArtists>,
) -> Vec<CreditedArtist> {
    let Some(tagged) = tagged else {
        return Vec::new();
    };
    let distinct: HashSet<String> = tagged
        .names
        .iter()
        .map(|name| name.to_lowercase())
        .collect();
    let usable = (2..=MAX_ARTISTS).contains(&tagged.names.len())
        && distinct.len() == tagged.names.len()
        && tagged
            .names
            .iter()
            .all(|name| name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control));
    if !usable {
        tracing::debug!(
            count = tagged.names.len(),
            "Tagged artist list is not a usable credit"
        );
        return Vec::new();
    }
    let ids: Vec<String> = tagged.ids.iter().filter_map(|id| identifier(id)).collect();
    let aligned = ids.len() == tagged.names.len() && ids.len() == tagged.ids.len();
    let named = credit_feature_names(display);
    let featured: Vec<bool> = tagged
        .names
        .iter()
        .map(|name| named.contains(&name.to_lowercase()))
        .collect();
    let all_featured = featured.iter().all(|featured| *featured);
    tracing::debug!(
        count = tagged.names.len(),
        identified = aligned,
        "Tagged artist list read as a credit"
    );
    tagged
        .names
        .iter()
        .enumerate()
        .map(|(index, name)| CreditedArtist {
            name: name.clone(),
            provider_artist_id: aligned.then(|| ids[index].clone()),
            page_url: None,
            role: if featured[index] && !all_featured {
                CreditRole::Featured
            } else {
                CreditRole::Main
            },
            join: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::tag::{ItemValue, TagItem, TagType};

    #[test]
    fn a_tag_with_thousands_of_values_is_cut_to_one_more_than_the_limit() {
        let mut tag = Tag::new(TagType::VorbisComments);
        for index in 0..5000 {
            tag.push(TagItem::new(
                ItemKey::TrackArtists,
                ItemValue::Text(format!("Artist {index}")),
            ));
        }
        assert_eq!(values(&tag, &ItemKey::TrackArtists).len(), MAX_ARTISTS + 1);
    }

    #[test]
    fn a_huge_value_is_cut_to_just_over_the_name_limit() {
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.push(TagItem::new(
            ItemKey::TrackArtists,
            ItemValue::Text("a".repeat(1_000_000)),
        ));
        assert_eq!(
            values(&tag, &ItemKey::TrackArtists)[0].len(),
            MAX_NAME_BYTES + 1
        );
    }

    #[test]
    fn a_panicking_read_degrades_to_no_tagged_artists() {
        let read: Option<TaggedArtists> = guarded(|| panic!("a malformed tag"));
        assert!(read.is_none());
    }
}
