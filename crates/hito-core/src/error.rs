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
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Upstream(&'static str),
    #[error("Database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("Database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("{0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Upstream(_) => "upstream",
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
