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
        }
    }
}
