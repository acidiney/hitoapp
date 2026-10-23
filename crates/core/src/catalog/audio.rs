pub(crate) mod headers;
pub(crate) mod local;
pub(crate) mod managed;
pub(crate) mod origin;
pub(crate) mod podcast;
pub(crate) mod provider;
mod range;
pub(crate) mod remote;
mod stream;
use super::Catalog;
use crate::{Actor, PlaybackSource, Result};
pub use range::ByteRange;
pub use stream::AudioStream;

#[derive(Default, Clone, Copy)]
pub struct AudioRequest {
    pub head: bool,
    pub range: Option<ByteRange>,
}

pub enum OpenedAudio {
    Content(Box<AudioContent>),
    Unsatisfiable { length: Option<u64> },
}

pub struct AudioContent {
    pub content_type: &'static str,
    pub length: Option<u64>,
    pub total_length: Option<u64>,
    pub range: Option<std::ops::RangeInclusive<u64>>,
    pub seekable: bool,
    pub body: Option<AudioStream>,
}

pub struct AudioFile {
    pub file: std::fs::File,
    pub length: u64,
    pub content_type: &'static str,
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn open_audio(
        &self,
        actor: &Actor,
        source: &str,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        if let Some(opened) = self
            .origins
            .provider
            .open_current(actor, source, request)
            .await?
        {
            return Ok(opened);
        }
        self.audio_playback_source(actor, source)
            .await?
            .open(request)
            .await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), source_id))]
    pub async fn audio_file(&self, actor: &Actor, source: &str) -> Result<AudioFile> {
        actor.check_session(&self.store).await?;
        for origin in self.origins.iter() {
            if let Some(file) = origin.open_file(actor, source).await? {
                return Ok(file);
            }
        }
        Err(crate::Error::NotFound)
    }
}

pub(super) async fn open_file_audio(
    grant: &PlaybackSource,
    audio: AudioFile,
    request: AudioRequest,
) -> Result<OpenedAudio> {
    let total = audio.length;
    let range = if request.head {
        None
    } else if let Some(selection) = request.range {
        let Some(range) = selection.resolve(total) else {
            return Ok(OpenedAudio::Unsatisfiable {
                length: Some(total),
            });
        };
        Some(range)
    } else {
        None
    };
    let start = range.as_ref().map_or(0, |range| *range.start());
    let length = range
        .as_ref()
        .map_or(total, |range| range.end() - range.start() + 1);
    let content_type = audio.content_type;
    let body = if request.head {
        None
    } else {
        Some(AudioStream::local(audio, grant.clone(), start, length).await?)
    };
    tracing::debug!(length, "Local audio opened");
    Ok(OpenedAudio::Content(Box::new(AudioContent {
        content_type,
        length: Some(length),
        total_length: Some(total),
        range,
        seekable: true,
        body,
    })))
}
