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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    OperationRejected,
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
            Error::Conflict(_) | Error::ProviderBusy(_) | Error::OperationRejected(_) => {
                StatusCode::CONFLICT
            }
            Error::Upstream(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let code =
            matches!(&self.0, Error::OperationRejected(_)).then_some(ErrorCode::OperationRejected);
        let error = if let Error::Upstream(message) = &self.0 {
            tracing::warn!(
                event = "upstream_failed",
                error_kind = self.0.code(),
                status = status.as_u16(),
                "External service failed"
            );
            message.to_string()
        } else if status.is_server_error() {
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
        (status, Json(ErrorResponse { error, code })).into_response()
    }
}

pub fn response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
            code: None,
        }),
    )
        .into_response()
}
