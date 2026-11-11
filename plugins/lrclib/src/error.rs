use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub(crate) enum Error {
    InvalidRequest,
    InvalidResponse,
    Busy,
    Unavailable,
    NotFound,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "The lyric provider request is invalid",
            ),
            Self::InvalidResponse => (
                StatusCode::BAD_GATEWAY,
                "The lyric provider returned invalid data",
            ),
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The lyric provider is busy. Try again later.",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The lyric provider is unavailable",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "The lyric provider record was not found",
            ),
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}
