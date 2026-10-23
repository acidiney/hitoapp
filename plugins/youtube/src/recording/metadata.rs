use super::Resolved;
use crate::Error;
use hito_plugin_sdk::RecordingMetadata;
use serde_json::Value;

pub(super) struct Musical {
    title: Option<String>,
    album: Option<String>,
}

impl Musical {
    pub(super) fn read(title: Value, album: Value) -> Result<Self, Error> {
        Ok(Self {
            title: text(title)?,
            album: text(album)?,
        })
    }
}

impl Resolved {
    pub(crate) fn metadata(&self) -> Result<RecordingMetadata, Error> {
        let musical = self.musical.as_ref().map_err(|error| *error)?;
        let metadata = RecordingMetadata {
            recording: self.recording.clone(),
            musical_title: musical.title.clone(),
            album_title: musical.album.clone(),
            source_ids: Some(vec![self.source.id.clone()]),
        };
        metadata.validate().map_err(|_| Error::InvalidResponse)?;
        Ok(metadata)
    }
}

fn text(value: Value) -> Result<Option<String>, Error> {
    match value {
        Value::Null => Ok(None),
        Value::String(value)
            if value.len() <= 2048
                && !value.trim().is_empty()
                && !value.chars().any(char::is_control) =>
        {
            Ok(Some(value))
        }
        _ => Err(Error::InvalidResponse),
    }
}
