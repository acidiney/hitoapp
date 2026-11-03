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
    Locked,
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
            Error::Locked => StatusCode::LOCKED,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            Error::Conflict(_) | Error::ProviderBusy(_) | Error::OperationRejected(_) => {
                StatusCode::CONFLICT
            }
            Error::Upstream(_) => StatusCode::BAD_GATEWAY,
            Error::InsufficientStorage => StatusCode::INSUFFICIENT_STORAGE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let code = match &self.0 {
            Error::OperationRejected(_) => Some(ErrorCode::OperationRejected),
            Error::Locked => Some(ErrorCode::Locked),
            _ => None,
        };
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
        let retry_after = match &self.0 {
            Error::RateLimited(seconds) => Some([("retry-after", seconds.to_string())]),
            _ => None,
        };
        (status, retry_after, Json(ErrorResponse { error, code })).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn a_full_disk_is_507_with_a_generic_body_and_no_paths() {
        let response = ApiError(hito_core::Error::InsufficientStorage).into_response();
        assert_eq!(response.status(), StatusCode::INSUFFICIENT_STORAGE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(body, r#"{"error":"Hito could not complete this request"}"#);
    }
}
