use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

pub struct ApiError(pub hito_core::Error);
pub type ApiResult<T> = Result<T, ApiError>;

impl From<hito_core::Error> for ApiError {
    fn from(error: hito_core::Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        use hito_core::Error;
        let status = match &self.0 {
            Error::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Forbidden => StatusCode::FORBIDDEN,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Conflict(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let error = if status.is_server_error() {
            tracing::error!(
                event = "request_failed",
                error_kind = self.0.code(),
                status = status.as_u16(),
                "Request failed"
            );
            "Hito could not complete this request".to_string()
        } else {
            tracing::debug!(
                event = "request_rejected",
                error_kind = self.0.code(),
                status = status.as_u16(),
                "Request rejected"
            );
            self.0.to_string()
        };
        (status, Json(ErrorResponse { error })).into_response()
    }
}

pub fn response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
        .into_response()
}
