#[derive(Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct PluginInstallationStamp {
    pub installation_id: String,
    pub installation_revision: i64,
}

#[derive(thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("Sign in to continue")]
    Unauthorized,
    #[error("You do not have access to this resource")]
    Forbidden,
    #[error("Resource not found")]
    NotFound,
    #[error("Enter your PIN to unlock this session")]
    Locked,
    #[error("That password is not correct")]
    WrongPassword,
    #[error("Too many attempts. Try again later.")]
    RateLimited(u64),
    #[error("Artwork requests are full. Try again after pending requests finish.")]
    ArtworkAdmissionBusy,
    #[error("Artwork storage is full. Existing artwork has been retained.")]
    ArtworkStorageFull,
    #[error("{0}")]
    Conflict(String),
    #[error("This source is already used by another track")]
    AlreadyBound(Option<String>),
    #[error("{0}")]
    ProviderBusy(&'static str),
    #[error("{0}")]
    AuthorizationRequired(&'static str),
    #[error("Update this plugin connection's service credentials in Settings.")]
    PluginCredentialsRequired(Option<PluginInstallationStamp>),
    #[error("{0}")]
    OperationRejected(String),
    #[error("{0}")]
    Upstream(&'static str),
    #[error("{0}")]
    UpstreamNotice(String),
    #[error("A playback provider request failed")]
    PlaybackProvider {
        reason: crate::PlaybackFailureReason,
        original: Box<Error>,
    },
    #[error("The audio service sent data too slowly")]
    StreamTooSlow,
    #[error("There is not enough disk space to save this audio")]
    InsufficientStorage,
    #[error("FFmpeg is not installed")]
    MediaToolsMissing,
    #[error("This audio format is not supported by the native decoder")]
    UnsupportedAudio,
    #[error("Database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("Database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("{0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn is_storage_full(&self) -> bool {
        match self {
            Self::InsufficientStorage => true,
            Self::Database(sqlx::Error::Database(error)) => error.code().as_deref() == Some("13"),
            _ => false,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Locked => "locked",
            Self::WrongPassword => "wrong_password",
            Self::RateLimited(_) => "rate_limited",
            Self::ArtworkAdmissionBusy => "artwork_admission_busy",
            Self::ArtworkStorageFull => "artwork_storage_full",
            Self::Conflict(_) => "conflict",
            Self::AlreadyBound(_) => "already_bound",
            Self::ProviderBusy(_) => "provider_busy",
            Self::AuthorizationRequired(_) => "authorization_required",
            Self::PluginCredentialsRequired(_) => "plugin_credentials_required",
            Self::OperationRejected(_) => "operation_rejected",
            Self::Upstream(_) | Self::UpstreamNotice(_) => "upstream",
            Self::PlaybackProvider { original, .. } => original.code(),
            Self::StreamTooSlow => "stream_too_slow",
            Self::InsufficientStorage => "insufficient_storage",
            Self::MediaToolsMissing => "media_tools_missing",
            Self::UnsupportedAudio => "unsupported_audio",
            Self::Database(_) => "database",
            Self::Migration(_) => "migration",
            Self::Internal(_) => "internal",
        }
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Error")
            .field("code", &self.code())
            .finish()
    }
}
