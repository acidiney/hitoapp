use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::server::AuthFailure;

pub(crate) enum Error {
    Unauthorized,
    InvalidRequest,
    NotFound,
    Busy,
    Deadline,
    SourceUnavailable,
    SourceChanged,
    UnrestrictionUncertain,
    ProviderUnavailable,
}

impl From<hito_debrid_real_debrid::Error> for Error {
    fn from(error: hito_debrid_real_debrid::Error) -> Self {
        use hito_debrid_real_debrid::Error as Provider;
        match error {
            Provider::Busy => Self::Busy,
            Provider::Deadline => Self::Deadline,
            Provider::SourceChanged => Self::SourceChanged,
            Provider::UnsupportedState => Self::SourceUnavailable,
            Provider::UnrestrictionUncertain => Self::UnrestrictionUncertain,
            _ => Self::ProviderUnavailable,
        }
    }
}

impl From<AuthFailure> for Error {
    fn from(failure: AuthFailure) -> Self {
        match failure {
            AuthFailure::Unauthorized => Self::Unauthorized,
            AuthFailure::InvalidProtocol => Self::InvalidRequest,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, reason, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Plugin-service authentication is required",
            ),
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The plugin request is invalid",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "The curated recording or source was not found",
            ),
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "capacity",
                "The provider is busy",
            ),
            Self::Deadline => (
                StatusCode::GATEWAY_TIMEOUT,
                "deadline",
                "The provider request exceeded its deadline",
            ),
            Self::SourceUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "source_unavailable",
                "The curated source is unavailable",
            ),
            Self::SourceChanged => (
                StatusCode::BAD_GATEWAY,
                "source_changed",
                "The provider file no longer matches the curated source",
            ),
            Self::UnrestrictionUncertain => (
                StatusCode::BAD_GATEWAY,
                "unrestriction_uncertain",
                "The provider acknowledgement is uncertain",
            ),
            Self::ProviderUnavailable => (
                StatusCode::BAD_GATEWAY,
                "provider_unavailable",
                "The provider could not supply the curated source",
            ),
        };
        tracing::debug!(
            event = "real_debrid_plugin_request_failed",
            outcome = reason
        );
        (
            status,
            Json(serde_json::json!({"error": message, "reason": reason})),
        )
            .into_response()
    }
}
