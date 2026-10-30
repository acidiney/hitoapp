use crate::InitError;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::server::{RequestPolicy, ServiceAuth};
use zeroize::Zeroizing;

pub(crate) fn request_policy(service_bearer: String) -> Result<RequestPolicy, InitError> {
    let bearer = Zeroizing::new(service_bearer);
    let auth = ServiceAuth::new(&bearer).ok_or(InitError::InvalidServiceCredential)?;
    Ok(RequestPolicy::new(
        hito_plugin_sdk::request_telemetry!("torbox"),
        Some(auth),
    ))
}

pub(crate) enum Error {
    InvalidRequest,
    NotFound,
    Conflict,
    Unavailable,
    Busy,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The request is invalid"),
            Self::NotFound => (StatusCode::NOT_FOUND, "The requested source is unavailable"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "The operation belongs to another source",
            ),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "The source is unavailable"),
            Self::Busy => (StatusCode::TOO_MANY_REQUESTS, "The service is busy"),
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}
