pub(crate) mod headers;
pub(crate) mod pace;
mod range;
pub(crate) mod remote;
mod stream;
use crate::{AudioFile, PlaybackSource, Result};
pub use pace::StreamPace;
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
