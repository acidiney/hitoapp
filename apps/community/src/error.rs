use crate::wire::Rejection;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Community configuration is invalid")]
    InvalidConfiguration,
    #[error("Community state is unavailable or not private")]
    StateUnavailable,
    #[error("Community state is already owned")]
    StateOwned,
    #[error("Community storage is unavailable")]
    StorageUnavailable,
    #[error("Community authentication is required")]
    Unauthorized,
    #[error("Community input is invalid")]
    InvalidInput,
    #[error("Submission identifier already has different content")]
    SubmissionConflict,
    #[error("Correction revision has changed")]
    RevisionConflict,
    #[error("Community resource is absent")]
    NotFound,
    #[error("Community outcome rate limit reached")]
    RateLimited,
    #[error("Request input was rejected")]
    Input(StatusCode),
}

impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        Self::StorageUnavailable
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let rate_limited = matches!(self, Self::RateLimited);
        let (status, error) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::InvalidInput => (StatusCode::BAD_REQUEST, "invalid_input"),
            Self::SubmissionConflict => (StatusCode::CONFLICT, "submission_conflict"),
            Self::RevisionConflict => (StatusCode::CONFLICT, "revision_conflict"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::Input(status) => (status, "invalid_input"),
            _ => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        };
        tracing::debug!(
            event = "community_request_rejected",
            status = status.as_u16(),
            outcome = error
        );
        let mut response = (status, Json(Rejection { error })).into_response();
        if rate_limited {
            response
                .headers_mut()
                .insert("retry-after", "60".parse().unwrap());
        }
        response
    }
}
