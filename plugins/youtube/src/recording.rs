mod identity;
mod metadata;
mod source;
pub(crate) use source::Descriptor;

use crate::{Error, extractor::Extractor, recordings};
use hito_plugin_sdk::ProviderRecording;
use serde::Deserialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};

pub(crate) struct Resolved {
    pub(crate) recording: ProviderRecording,
    pub(crate) source: Descriptor,
    musical: Result<metadata::Musical, Error>,
}

pub(crate) struct Catalog {
    extractor: Arc<Extractor>,
    records: Mutex<VecDeque<Arc<Resolved>>>,
}

impl Catalog {
    pub(crate) fn new(extractor: Arc<Extractor>) -> Self {
        Self {
            extractor,
            records: Mutex::new(VecDeque::new()),
        }
    }

    pub(crate) async fn recording(&self, id: &str) -> Result<Arc<Resolved>, Error> {
        if !recordings::valid_id(id) {
            return Err(Error::InvalidRecordingRequest);
        }
        if !self.extractor.available() {
            return Err(Error::Unavailable);
        }
        {
            let mut records = self
                .records
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(index) = records.iter().position(|cached| cached.recording.id == id) {
                let cached = records.remove(index).ok_or(Error::Unavailable)?;
                let fresh = Instant::now() < cached.source.expires;
                records.push_back(cached.clone());
                if fresh {
                    return Ok(cached);
                }
            }
        }
        let mut resolved = self.extractor.recording(id.to_owned()).await?;
        if !self.extractor.available() {
            return Err(Error::Unavailable);
        }
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(index) = records.iter().position(|cached| cached.recording.id == id) {
            let previous = records.remove(index).ok_or(Error::Unavailable)?;
            resolved.source.renew(&previous.source);
        }
        let resolved = Arc::new(resolved);
        if records.len() >= 128 {
            records.pop_front();
        }
        records.push_back(resolved.clone());
        Ok(resolved)
    }

    pub(crate) async fn source(&self, id: &str) -> Result<Descriptor, Error> {
        let identity = identity::Identity::parse(id)?;
        let resolved = self.recording(identity.owner()).await?;
        if resolved.source.id != id {
            return Err(Error::NotFound);
        }
        Ok(resolved.source.clone())
    }
}

#[derive(Deserialize)]
struct Recording {
    #[serde(flatten)]
    metadata: recordings::SearchEntry,
    #[serde(default)]
    track: serde_json::Value,
    #[serde(default)]
    album: serde_json::Value,
    format_id: String,
    ext: String,
    acodec: String,
    vcodec: String,
    protocol: String,
    filesize: u64,
    url: String,
    is_live: Option<bool>,
    live_status: Option<String>,
}

pub(crate) fn resolve(bytes: &[u8], owner: &str) -> Result<Resolved, Error> {
    let details: Recording = serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    let musical = metadata::Musical::read(details.track, details.album);
    let recording = details.metadata.recording()?;
    if recording.id != owner
        || details.ext != "m4a"
        || details.acodec != "mp4a.40.2"
        || details.vcodec != "none"
        || details.protocol != "https"
        || details.is_live == Some(true)
        || details
            .live_status
            .as_deref()
            .is_some_and(|value| value != "not_live")
    {
        return Err(Error::InvalidResponse);
    }
    let source = Descriptor::new(owner, &details.format_id, details.filesize, &details.url)?;
    Ok(Resolved {
        recording,
        source,
        musical,
    })
}
