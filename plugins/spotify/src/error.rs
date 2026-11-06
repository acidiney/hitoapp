use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("The playlist request is invalid")]
    InvalidRequest,
    #[error("Spotify does not allow public access to this playlist")]
    PublicAccessDenied,
    #[error(
        "Spotify does not serve this playlist to a public link. Private playlists and personal mixes such as Discover Weekly need your Spotify account and cannot be imported"
    )]
    NotPublic,
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
    #[error("The artwork request is invalid")]
    InvalidArtworkRequest,
    #[error("Spotify returned invalid artwork")]
    InvalidArtworkResponse,
    #[error("The Spotify artwork was not found")]
    ArtworkNotFound,
    #[error("Spotify does not allow public access to this artwork")]
    ArtworkAccessDenied,
    #[error("The artwork request timed out")]
    ArtworkDeadline,
}

impl Error {
    pub(crate) fn for_artwork(self) -> Self {
        match self {
            Self::InvalidRequest => Self::InvalidArtworkRequest,
            Self::InvalidResponse => Self::InvalidArtworkResponse,
            Self::NotFound | Self::NotPublic => Self::ArtworkNotFound,
            Self::PublicAccessDenied => Self::ArtworkAccessDenied,
            Self::Deadline => Self::ArtworkDeadline,
            other => other,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidRequest | Self::InvalidArtworkRequest => StatusCode::BAD_REQUEST,
            Self::PublicAccessDenied | Self::ArtworkAccessDenied => StatusCode::FORBIDDEN,
            Self::NotFound | Self::ArtworkNotFound => StatusCode::NOT_FOUND,
            Self::NotPublic => StatusCode::UNPROCESSABLE_ENTITY,
            Self::SnapshotUnavailable => StatusCode::GONE,
            Self::Busy | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::InvalidResponse | Self::InvalidArtworkResponse => StatusCode::BAD_GATEWAY,
            Self::Deadline | Self::ArtworkDeadline => StatusCode::GATEWAY_TIMEOUT,
        };
        (status, Json(serde_json::json!({"error": self.to_string()}))).into_response()
    }
}
