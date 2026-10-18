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
            Self::Status(_) => "provider_rejected",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidResponse => "invalid_response",
            Self::SourceChanged => "source_changed",
            Self::UnsupportedState => "unsupported_state",
        }
    }
}
