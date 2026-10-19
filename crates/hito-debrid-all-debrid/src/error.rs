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
        }
    }
}
