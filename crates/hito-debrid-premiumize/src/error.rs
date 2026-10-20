#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderRejection {
    AuthenticationFailed,
    PermissionDenied,
    RateLimited,
    NotFound,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("The Premiumize client configuration is invalid")]
    Configuration,
    #[error("The Premiumize provider credential is invalid")]
    InvalidCredential,
    #[error("The configured Premiumize transfer binding is invalid")]
    InvalidBinding,
    #[error("The Premiumize client is busy")]
    Busy,
    #[error("The Premiumize observation exceeded its deadline")]
    Deadline,
    #[error("The Premiumize API request failed")]
    Transport,
    #[error("The Premiumize API rejected the HTTP request")]
    Status(u16),
    #[error("The Premiumize response exceeds its bound")]
    ResponseTooLarge,
    #[error("The Premiumize response is invalid")]
    InvalidResponse,
    #[error("The Premiumize provider rejected the observation")]
    ProviderRejected(ProviderRejection),
    #[error("The configured Premiumize transfer is absent")]
    TransferNotFound,
    #[error("The Premiumize transfer no longer matches the configured reference")]
    TransferChanged,
    #[error("The Premiumize transfer state is not supported")]
    UnsupportedState,
    #[error("The Premiumize file no longer matches the configured reference")]
    FileChanged,
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
            Self::InvalidCredential => "invalid_credential",
            Self::InvalidBinding => "invalid_binding",
            Self::Busy => "busy",
            Self::Deadline => "deadline",
            Self::Transport => "transport",
            Self::Status(_) => "http_rejected",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidResponse => "invalid_response",
            Self::ProviderRejected(_) => "provider_rejected",
            Self::TransferNotFound => "transfer_not_found",
            Self::TransferChanged => "transfer_changed",
            Self::UnsupportedState => "unsupported_state",
            Self::FileChanged => "file_changed",
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
