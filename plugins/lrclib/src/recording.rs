use crate::error::Error;
use hito_plugin_sdk::ProviderRecording;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Record {
    pub id: u64,
    pub track_name: String,
    pub artist_name: String,
    pub duration: Option<f64>,
    pub album_name: Option<String>,
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

impl Record {
    pub(crate) fn recording(&self) -> Result<ProviderRecording, Error> {
        if self.id == 0 {
            return Err(Error::InvalidResponse);
        }
        if self
            .album_name
            .as_deref()
            .is_some_and(|title| title.len() > 2048 || title.chars().any(char::is_control))
        {
            return Err(Error::InvalidResponse);
        }
        let duration_ms = match self.duration {
            None | Some(0.0) => None,
            Some(duration) if duration.is_finite() && (0.0..=604_800.0).contains(&duration) => {
                Some((duration * 1000.0).round() as u64)
            }
            _ => return Err(Error::InvalidResponse),
        };
        let recording = ProviderRecording {
            id: self.id.to_string(),
            title: self.track_name.clone(),
            artist: self.artist_name.clone(),
            duration_ms,
            release: None,
            disc_number: None,
            track_number: None,
            revision: None,
            attribution: None,
            isrc: None,
            artist_urls: Vec::new(),
            artists: Vec::new(),
        };
        recording.validate().map_err(|_| Error::InvalidResponse)?;
        Ok(recording)
    }
}
