use super::probe::Probe;
use crate::metadata::{MusicBrainzArtistRole, MusicBrainzArtistTag, TagLocation};

const MAX_ASSERTIONS: usize = 16;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 2048;
const MAX_AGGREGATE_BYTES: usize = 32 * 1024;

pub(super) fn collect(probe: &Probe) -> Option<Vec<MusicBrainzArtistTag>> {
    let locations = std::iter::once((&probe.format.tags, TagLocation::Format)).chain(
        probe
            .streams
            .iter()
            .enumerate()
            .map(|(index, stream)| (&stream.tags, TagLocation::AudioStream { index })),
    );
    let mut assertions = Vec::new();
    let mut encoded_bytes = 2;
    for (tags, location) in locations {
        for (key, value) in tags {
            let Some(role) = role(key) else {
                continue;
            };
            let reason = if assertions.len() >= MAX_ASSERTIONS {
                Some("assertion_limit")
            } else if key.len() > MAX_KEY_BYTES {
                Some("key_byte_limit")
            } else if value.len() > MAX_VALUE_BYTES {
                Some("value_byte_limit")
            } else {
                None
            };
            if let Some(reason) = reason {
                tracing::debug!(
                    reason,
                    assertion_count = assertions.len() + 1,
                    "MusicBrainz artist tag assertions unavailable"
                );
                return None;
            }
            let assertion = MusicBrainzArtistTag {
                role,
                key: key.clone(),
                value: value.clone(),
                location,
            };
            let Ok(encoded) = serde_json::to_vec(&assertion) else {
                tracing::debug!(
                    reason = "encoding",
                    "MusicBrainz artist tag assertions unavailable"
                );
                return None;
            };
            encoded_bytes += encoded.len() + usize::from(!assertions.is_empty());
            if encoded_bytes > MAX_AGGREGATE_BYTES {
                tracing::debug!(
                    reason = "aggregate_byte_limit",
                    assertion_count = assertions.len() + 1,
                    "MusicBrainz artist tag assertions unavailable"
                );
                return None;
            }
            assertions.push(assertion);
        }
    }
    tracing::debug!(
        assertion_count = assertions.len(),
        encoded_bytes,
        "MusicBrainz artist tag assertions retained"
    );
    Some(assertions)
}

fn role(key: &str) -> Option<MusicBrainzArtistRole> {
    if key.eq_ignore_ascii_case("MusicBrainz Artist Id")
        || key.eq_ignore_ascii_case("MUSICBRAINZ_ARTISTID")
    {
        Some(MusicBrainzArtistRole::Artist)
    } else if key.eq_ignore_ascii_case("MusicBrainz Album Artist Id")
        || key.eq_ignore_ascii_case("MUSICBRAINZ_ALBUMARTISTID")
    {
        Some(MusicBrainzArtistRole::AlbumArtist)
    } else {
        None
    }
}
