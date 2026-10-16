use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("The playlist request is invalid")]
    InvalidRequest,
    #[error("Connect your Spotify account to read this playlist")]
    Unauthorized,
    #[error("Spotify does not allow this account to read the playlist contents")]
    AccessDenied,
    #[error("Spotify does not allow public access to this playlist")]
    PublicAccessDenied,
    #[error("The Spotify playlist was not found")]
    NotFound,
    #[error("This playlist snapshot is no longer available. Import the playlist again")]
    SnapshotUnavailable,
    #[error("The Spotify service is busy. Try again later")]
    Busy,
    #[error("Spotify could not be reached")]
    Unavailable,
    #[error("Spotify returned an invalid playlist response")]
    InvalidResponse,
    #[error("The playlist request timed out")]
    Deadline,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::AccessDenied | Self::PublicAccessDenied => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::SnapshotUnavailable => StatusCode::GONE,
            Self::Busy | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::InvalidResponse => StatusCode::BAD_GATEWAY,
            Self::Deadline => StatusCode::GATEWAY_TIMEOUT,
        };
        (status, Json(serde_json::json!({"error": self.to_string()}))).into_response()
    }
}
