use crate::error::ErrorResponse;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hito_playback::PlayerError;

pub struct Error(PlayerError);
impl From<PlayerError> for Error {
    fn from(error: PlayerError) -> Self {
        Self(error)
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self.0 {
            PlayerError::Unauthorized => StatusCode::UNAUTHORIZED,
            PlayerError::Conflict => StatusCode::CONFLICT,
            PlayerError::Busy => StatusCode::TOO_MANY_REQUESTS,
            PlayerError::Closed | PlayerError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            PlayerError::NotFound => StatusCode::NOT_FOUND,
            PlayerError::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
            PlayerError::Unsupported => StatusCode::NOT_IMPLEMENTED,
            PlayerError::Playback | PlayerError::OutputFormat => StatusCode::BAD_GATEWAY,
            PlayerError::Persistence | PlayerError::Resume => StatusCode::INTERNAL_SERVER_ERROR,
        };
        tracing::debug!(event = "player_http_rejected", error_kind = ?self.0, status = status.as_u16(), "Playback request rejected");
        let mut response = (
            status,
            Json(ErrorResponse {
                error: self.0.to_string(),
                code: None,
                recording_id: None,
                plugin_connection: None,
            }),
        )
            .into_response();
        if status == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert("retry-after", "1".parse().unwrap());
        }
        response
    }
}
