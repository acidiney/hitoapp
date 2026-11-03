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
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    ProviderBusy(&'static str),
    #[error("{0}")]
    OperationRejected(String),
    #[error("{0}")]
    Upstream(&'static str),
    #[error("There is not enough disk space to save this audio")]
    InsufficientStorage,
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
            Self::Conflict(_) => "conflict",
            Self::ProviderBusy(_) => "provider_busy",
            Self::OperationRejected(_) => "operation_rejected",
            Self::Upstream(_) => "upstream",
            Self::InsufficientStorage => "insufficient_storage",
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
