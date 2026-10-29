#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    #[error("The audio format is unsupported")]
    Unsupported,
    #[error("Audio could not be decoded")]
    Decode,
    #[error("The audio format changed during playback")]
    FormatChanged,
    #[error("Audio could not seek to the requested position")]
    Seek,
    #[error("A compatible audio output device is unavailable")]
    OutputUnavailable,
    #[error("This audio device can't play this file's format")]
    OutputFormat,
    #[error("The audio output device failed")]
    Output,
    #[error("The audio worker failed")]
    Worker,
    #[error("The audio control value is invalid")]
    InvalidControl,
    #[error("Audio reading was interrupted")]
    Interrupted,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<symphonia::core::errors::Error> for Error {
    fn from(value: symphonia::core::errors::Error) -> Self {
        match value {
            symphonia::core::errors::Error::Unsupported(_) => Self::Unsupported,
            _ => Self::Decode,
        }
    }
}
