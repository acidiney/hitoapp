#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("The Real-Debrid provider credential is invalid")]
    InvalidCredential,
    #[error("The Real-Debrid client configuration is invalid")]
    Configuration,
    #[error("The Real-Debrid file binding is invalid")]
    InvalidBinding,
    #[error("The Real-Debrid client is busy")]
    Busy,
    #[error("The Real-Debrid observation exceeded its deadline")]
    Deadline,
    #[error("The Real-Debrid service could not be reached")]
    Transport,
    #[error("The Real-Debrid unrestriction acknowledgement is uncertain")]
    UnrestrictionUncertain,
    #[error("The Real-Debrid service rejected the request")]
    Status(u16),
    #[error("The Real-Debrid response exceeds the size limit")]
    ResponseTooLarge,
    #[error("The Real-Debrid service returned an invalid observation")]
    InvalidResponse,
    #[error("The Real-Debrid source no longer matches the bound file")]
    SourceChanged,
    #[error("The Real-Debrid torrent state is not supported")]
    UnsupportedState,
    #[error("The bound audio destination is not permitted")]
    MediaDestination,
    #[error("The bound audio connection failed")]
    MediaTransport,
    #[error("The bound audio response is invalid")]
    MediaResponse,
    #[error("The bound audio body exceeded its idle deadline")]
    MediaIdle,
    #[error("The bound audio does not match its declared supported format")]
    UnsupportedAudio,
    #[error("The bound audio body does not match its confirmed length")]
    MediaLength,
}

impl Error {
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::InvalidCredential => "invalid_credential",
            Self::Configuration => "configuration",
            Self::InvalidBinding => "invalid_binding",
            Self::Busy => "busy",
            Self::Deadline => "deadline",
            Self::Transport => "transport",
            Self::UnrestrictionUncertain => "unrestriction_uncertain",
            Self::Status(_) => "provider_rejected",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidResponse => "invalid_response",
            Self::SourceChanged => "source_changed",
            Self::UnsupportedState => "unsupported_state",
            Self::MediaDestination => "media_destination",
            Self::MediaTransport => "media_transport",
            Self::MediaResponse => "media_response",
            Self::MediaIdle => "media_idle",
            Self::UnsupportedAudio => "unsupported_audio",
            Self::MediaLength => "media_length",
        }
    }
}

impl From<hito_remote_audio::Error> for Error {
    fn from(error: hito_remote_audio::Error) -> Self {
        match error {
            hito_remote_audio::Error::Configuration => Self::Configuration,
            hito_remote_audio::Error::Busy => Self::Busy,
            hito_remote_audio::Error::MediaDestination => Self::MediaDestination,
            hito_remote_audio::Error::MediaTransport => Self::MediaTransport,
            hito_remote_audio::Error::MediaResponse => Self::MediaResponse,
            hito_remote_audio::Error::MediaIdle => Self::MediaIdle,
            hito_remote_audio::Error::UnsupportedAudio => Self::UnsupportedAudio,
            hito_remote_audio::Error::MediaLength => Self::MediaLength,
        }
    }
}
