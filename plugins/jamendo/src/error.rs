use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Clone, Copy)]
pub(crate) enum Error {
    InvalidRequest,
    InvalidRange,
    NotFound,
    Stale,
    Busy,
    Unavailable,
    InvalidResponse,
    Deadline,
}

impl Error {
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::InvalidRequest | Self::InvalidRange => "invalid_request",
            Self::NotFound => "not_found",
            Self::Stale => "stale",
            Self::Busy => "capacity",
            Self::Unavailable => "unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::Deadline => "deadline",
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The Jamendo request is invalid"),
            Self::InvalidRange => (
                StatusCode::RANGE_NOT_SATISFIABLE,
                "The audio range is unsupported",
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "The Jamendo source was not found"),
            Self::Stale => (StatusCode::CONFLICT, "The Jamendo album changed"),
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The Jamendo service is busy. Try again later",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The Jamendo service is unavailable",
            ),
            Self::InvalidResponse => (
                StatusCode::BAD_GATEWAY,
                "Jamendo returned an invalid response",
            ),
            Self::Deadline => (StatusCode::GATEWAY_TIMEOUT, "The Jamendo request timed out"),
        };
        tracing::debug!(event = "jamendo_request_failed", outcome = self.outcome());
        (
            status,
            [("cache-control", "private, no-store")],
            Json(serde_json::json!({"error": message})),
        )
            .into_response()
    }
}
