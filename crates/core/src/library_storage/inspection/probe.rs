use crate::metadata::{InspectedTags, ObservedTag, TagLocation};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub(super) struct Probe {
    #[serde(default)]
    pub streams: Vec<Stream>,
    pub format: Format,
}

#[derive(Deserialize)]
pub(super) struct Stream {
    pub codec_name: Option<String>,
    pub sample_rate: Option<String>,
    pub channels: Option<u32>,
    pub channel_layout: Option<String>,
    pub bits_per_sample: Option<u32>,
    pub bits_per_raw_sample: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub(super) struct Format {
    pub format_name: Option<String>,
    pub duration: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

impl Probe {
    pub(super) fn mime(&self) -> Option<&'static str> {
        match self.format.format_name.as_deref()?.split(',').next()? {
            "wav" => Some("audio/wav"),
            "flac" => Some("audio/flac"),
            "mp3" => Some("audio/mpeg"),
            "ogg" => Some("audio/ogg"),
            "mov" => Some("audio/mp4"),
            "aac" => Some("audio/aac"),
            "aiff" => Some("audio/aiff"),
            "asf" => Some("audio/x-ms-wma"),
            _ => None,
        }
    }

    pub(super) fn number(&self, name: &str) -> Option<i64> {
        self.tag(name)?
            .split('/')
            .next()?
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|number| *number > 0)
            .map(i64::from)
    }

    pub(super) fn tag(&self, name: &str) -> Option<String> {
        self.observed_tag(name).map(|tag| tag.value)
    }

    pub(super) fn tags(
        &self,
        artist_tags_known: bool,
        id3_recording_id: Option<String>,
    ) -> InspectedTags {
        InspectedTags {
            title: self.observed_tag("title"),
            artist: self.observed_tag("artist"),
            album: self.observed_tag("album"),
            album_artist: self.observed_tag("album_artist"),
            disc: self.observed_tag("disc"),
            track: self.observed_tag("track"),
            isrc: self
                .observed_tag("isrc")
                .or_else(|| self.observed_tag("tsrc")),
            genre: self.observed_tag("genre"),
            musicbrainz_album_id: self.first_tag(&["musicbrainz album id", "musicbrainz_albumid"]),
            musicbrainz_release_group_id: self
                .first_tag(&["musicbrainz release group id", "musicbrainz_releasegroupid"]),
            musicbrainz_recording_id: if self.carries_id3() {
                id3_recording_id.map(|value| ObservedTag {
                    value,
                    location: TagLocation::Format,
                })
            } else {
                self.first_tag(&["musicbrainz track id", "musicbrainz_trackid"])
            },
            musicbrainz_release_track_id: self
                .first_tag(&["musicbrainz release track id", "musicbrainz_releasetrackid"]),
            musicbrainz_artist_tags: if artist_tags_known {
                super::artist_tags::collect(self)
            } else {
                None
            },
        }
    }

    /// An ID3 "MusicBrainz Track Id" text frame is ambiguous (taggers disagree on whether it names
    fn carries_id3(&self) -> bool {
        matches!(
            self.format
                .format_name
                .as_deref()
                .and_then(|name| name.split(',').next()),
            Some("mp3" | "wav" | "aiff" | "aac")
        )
    }

    fn first_tag(&self, names: &[&str]) -> Option<ObservedTag> {
        names.iter().find_map(|name| self.observed_tag(name))
    }

    fn observed_tag(&self, name: &str) -> Option<ObservedTag> {
        let find = |tags: &BTreeMap<String, String>, location| {
            tags.iter()
                .find(|(key, value)| key.eq_ignore_ascii_case(name) && !value.trim().is_empty())
                .map(|(_, value)| ObservedTag {
                    value: value.clone(),
                    location,
                })
        };
        find(&self.format.tags, TagLocation::Format).or_else(|| {
            self.streams
                .iter()
                .enumerate()
                .find_map(|(index, stream)| find(&stream.tags, TagLocation::AudioStream { index }))
        })
    }

    pub(super) fn profile(&self) -> Option<(&str, u32, u32, Option<&str>)> {
        if self.streams.len() != 1 {
            return None;
        }
        let stream = &self.streams[0];
        let codec = stream.codec_name.as_deref()?;
        let depth = stream
            .bits_per_raw_sample
            .as_deref()
            .and_then(|v| v.parse::<u32>().ok())
            .or(stream.bits_per_sample)
            .unwrap_or(0);
        if depth > 32 && !codec.starts_with("pcm_f64") {
            return None;
        }
        let rate = stream.sample_rate.as_deref()?.parse().ok()?;
        let channels = stream.channels?;
        if !(1..=768000).contains(&rate) || !(1..=64).contains(&channels) {
            return None;
        }
        Some((codec, rate, channels, stream.channel_layout.as_deref()))
    }

    pub(super) fn duration_ms(&self) -> i64 {
        self.format
            .duration
            .as_deref()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| (v * 1000.0) as i64)
            .unwrap_or(0)
    }
}
