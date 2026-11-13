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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_connection: Option<hito_core::PluginInstallationStamp>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    OperationRejected,
    Locked,
    WrongPassword,
    AlreadyBound,
    PluginCredentialsRequired,
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
            Error::Invalid(_) | Error::WrongPassword => StatusCode::UNPROCESSABLE_ENTITY,
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Forbidden => StatusCode::FORBIDDEN,
            Error::Locked => StatusCode::LOCKED,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::UnsupportedAudio => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Error::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            Error::Conflict(_)
            | Error::ProviderBusy(_)
            | Error::AuthorizationRequired(_)
            | Error::PluginCredentialsRequired(_)
            | Error::AlreadyBound(_)
            | Error::OperationRejected(_) => StatusCode::CONFLICT,
            Error::Upstream(_) | Error::UpstreamNotice(_) | Error::StreamTooSlow => {
                StatusCode::BAD_GATEWAY
            }
            Error::InsufficientStorage => StatusCode::INSUFFICIENT_STORAGE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let code = match &self.0 {
            Error::OperationRejected(_) => Some(ErrorCode::OperationRejected),
            Error::Locked => Some(ErrorCode::Locked),
            Error::WrongPassword => Some(ErrorCode::WrongPassword),
            Error::AlreadyBound(_) => Some(ErrorCode::AlreadyBound),
            Error::PluginCredentialsRequired(_) => Some(ErrorCode::PluginCredentialsRequired),
            _ => None,
        };
        let error = if matches!(
            &self.0,
            Error::Upstream(_) | Error::UpstreamNotice(_) | Error::StreamTooSlow
        ) {
            tracing::warn!(
                event = "upstream_failed",
                error_kind = self.0.code(),
                status = status.as_u16(),
                "External service failed"
            );
            match &self.0 {
                Error::Upstream(message) => message.to_string(),
                Error::UpstreamNotice(message) => message.clone(),
                other => other.to_string(),
            }
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
        let recording_id = match &self.0 {
            Error::AlreadyBound(recording) => recording.clone(),
            _ => None,
        };
        let plugin_connection = match &self.0 {
            Error::PluginCredentialsRequired(installation) => installation.clone(),
            _ => None,
        };
        (
            status,
            retry_after,
            Json(ErrorResponse {
                error,
                code,
                recording_id,
                plugin_connection,
            }),
        )
            .into_response()
    }
}

pub fn response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
            code: None,
            recording_id: None,
            plugin_connection: None,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn unsupported_native_audio_is_415_with_a_safe_message() {
        let response = ApiError(hito_core::Error::UnsupportedAudio).into_response();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            &body[..],
            br#"{"error":"This audio format is not supported by the native decoder"}"#
        );
    }

    #[tokio::test]
    async fn a_full_disk_is_507_with_a_generic_body_and_no_paths() {
        let response = ApiError(hito_core::Error::InsufficientStorage).into_response();
        assert_eq!(response.status(), StatusCode::INSUFFICIENT_STORAGE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(body, r#"{"error":"Hito could not complete this request"}"#);
    }
}
