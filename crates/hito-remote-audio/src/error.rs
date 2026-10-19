#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("The remote audio configuration is invalid")]
    Configuration,
    #[error("The remote audio client is busy")]
    Busy,
    #[error("The bound audio destination is not permitted")]
    MediaDestination,
    #[error("The bound audio connection failed")]
    MediaTransport,
    #[error("The bound audio response is invalid")]
    MediaResponse,
    #[error("The bound audio body exceeded its idle deadline")]
    MediaIdle,
    #[error("The bound audio is not a supported canonical PCM16 WAV")]
    UnsupportedAudio,
    #[error("The bound audio body does not match its confirmed length")]
    MediaLength,
}

impl Error {
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Busy => "busy",
            Self::MediaDestination => "media_destination",
            Self::MediaTransport => "media_transport",
            Self::MediaResponse => "media_response",
            Self::MediaIdle => "media_idle",
            Self::UnsupportedAudio => "unsupported_audio",
            Self::MediaLength => "media_length",
        }
    }
}
