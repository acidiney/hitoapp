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
    pub duration: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

impl Probe {
    pub(super) fn tag(&self, name: &str) -> Option<String> {
        self.format
            .tags
            .iter()
            .chain(self.streams.iter().flat_map(|stream| &stream.tags))
            .find(|(key, value)| key.eq_ignore_ascii_case(name) && !value.trim().is_empty())
            .map(|(_, value)| value.clone())
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
