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
        }
    }
}
