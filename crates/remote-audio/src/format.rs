use crate::{Error, Pcm16WavFormat, mp3, wav};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioKind {
    #[default]
    Pcm16Wav,
    Mp3,
}

impl AudioKind {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Pcm16Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
        }
    }

    pub(crate) fn prefix_limit(self) -> usize {
        match self {
            Self::Pcm16Wav => 44,
            Self::Mp3 => mp3::PREFIX_LIMIT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mp3Format {
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Pcm16Wav(Pcm16WavFormat),
    Mp3(Mp3Format),
}

impl AudioFormat {
    pub fn kind(self) -> AudioKind {
        match self {
            Self::Pcm16Wav(_) => AudioKind::Pcm16Wav,
            Self::Mp3(_) => AudioKind::Mp3,
        }
    }
}

pub(crate) fn qualify(
    kind: AudioKind,
    prefix: &[u8],
    length: u64,
) -> Result<Option<AudioFormat>, Error> {
    match kind {
        AudioKind::Pcm16Wav => match prefix.get(..44) {
            Some(header) => wav::qualify(
                header.try_into().map_err(|_| Error::UnsupportedAudio)?,
                length,
            )
            .map(AudioFormat::Pcm16Wav)
            .map(Some),
            None => Ok(None),
        },
        AudioKind::Mp3 => mp3::qualify(prefix, length).map(|format| format.map(AudioFormat::Mp3)),
    }
}
