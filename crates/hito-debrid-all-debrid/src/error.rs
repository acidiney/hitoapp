#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("The AllDebrid client configuration is invalid")]
    Configuration,
    #[error("The AllDebrid provider credential is invalid")]
    InvalidCredential,
    #[error("The configured AllDebrid file binding is invalid")]
    InvalidBinding,
    #[error("The AllDebrid client is busy")]
    Busy,
    #[error("The AllDebrid operation exceeded its deadline")]
    Deadline,
    #[error("The AllDebrid upload acknowledgment is uncertain")]
    SubmissionUnknown,
    #[error("The AllDebrid unlock acknowledgment is uncertain")]
    UnlockUnknown,
    #[error("The acknowledged AllDebrid generated link requires separate media validation")]
    GeneratedLinkUnsupported,
    #[error("The AllDebrid API request failed")]
    Transport,
    #[error("The AllDebrid API rejected the request")]
    Status(u16),
    #[error("The AllDebrid response exceeds its bound")]
    ResponseTooLarge,
    #[error("The AllDebrid response is invalid")]
    InvalidResponse,
    #[error("The AllDebrid provider rejected the operation")]
    ProviderRejected,
    #[error("The AllDebrid provider reported failed processing")]
    ProviderFailed,
    #[error("The AllDebrid source no longer matches the configured file")]
    SourceChanged,
    #[error("The AllDebrid observation state is not supported")]
    UnsupportedState,
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
            Self::SubmissionUnknown => "submission_unknown",
            Self::UnlockUnknown => "unlock_unknown",
            Self::GeneratedLinkUnsupported => "generated_link_unsupported",
            Self::Transport => "transport",
            Self::Status(_) => "http_rejected",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidResponse => "invalid_response",
            Self::ProviderRejected => "provider_rejected",
            Self::ProviderFailed => "provider_failed",
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
