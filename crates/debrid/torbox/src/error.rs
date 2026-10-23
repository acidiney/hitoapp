#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("The TorBox client configuration is invalid")]
    Configuration,
    #[error("The TorBox provider credential is invalid")]
    InvalidCredential,
    #[error("The configured TorBox file binding is invalid")]
    InvalidBinding,
    #[error("The TorBox client is busy")]
    Busy,
    #[error("The TorBox observation exceeded its deadline")]
    Deadline,
    #[error("The TorBox API request failed")]
    Transport,
    #[error("The TorBox API rejected the request")]
    Status(u16),
    #[error("The TorBox response exceeds its bound")]
    ResponseTooLarge,
    #[error("The TorBox response is invalid")]
    InvalidResponse,
    #[error("The TorBox provider rejected the observation")]
    ProviderRejected,
    #[error("The configured TorBox file is not complete and present")]
    SourceIncomplete,
    #[error("The TorBox generated-link acknowledgment is uncertain")]
    LinkUnknown,
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
    #[error("The TorBox source no longer matches the configured file")]
    SourceChanged,
}

impl Error {
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::InvalidCredential => "invalid_credential",
            Self::InvalidBinding => "invalid_binding",
            Self::Busy => "busy",
            Self::Deadline => "deadline",
            Self::Transport => "transport",
            Self::Status(_) => "http_rejected",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidResponse => "invalid_response",
            Self::ProviderRejected => "provider_rejected",
            Self::SourceChanged => "source_changed",
            Self::SourceIncomplete => "incomplete",
            Self::LinkUnknown => "link_unknown",
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
