use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::time::Instant;

pub(crate) enum UpstreamFailure {
    InvalidJson,
    HeaderCount,
    ContentType,
    ContentEncoding,
    AnnouncedBodyLimit,
    StreamedBodyLimit,
    HttpStatus(u16),
    RequestTransport,
    BodyTransport,
    Deadline,
    Cooldown(u16),
    InvalidRetryAfter(u16),
    RetryAfterOverflow(u16),
}

impl UpstreamFailure {
    fn diagnostic(&self) -> (&'static str, &'static str, Option<u16>) {
        match self {
            Self::InvalidJson => ("invalid_json", "decode", Some(200)),
            Self::HeaderCount => ("too_many_headers", "headers", Some(200)),
            Self::ContentType => ("invalid_content_type", "headers", Some(200)),
            Self::ContentEncoding => ("unsupported_content_encoding", "headers", Some(200)),
            Self::AnnouncedBodyLimit => ("announced_body_limit", "headers", Some(200)),
            Self::StreamedBodyLimit => ("streamed_body_limit", "body", Some(200)),
            Self::HttpStatus(status) => ("http_status", "response", Some(*status)),
            Self::RequestTransport => ("transport", "request", None),
            Self::BodyTransport => ("transport", "body", Some(200)),
            Self::Deadline => ("deadline_exceeded", "exchange", None),
            Self::Cooldown(status) => ("cooldown", "cooldown", Some(*status)),
            Self::InvalidRetryAfter(status) => ("invalid_retry_after", "cooldown", Some(*status)),
            Self::RetryAfterOverflow(status) => ("retry_after_overflow", "cooldown", Some(*status)),
        }
    }

    fn response(&self) -> (StatusCode, &'static str) {
        match self {
            Self::Cooldown(_) | Self::InvalidRetryAfter(_) | Self::RetryAfterOverflow(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The lyric provider is busy. Try again later.",
            ),
            Self::HttpStatus(_) | Self::RequestTransport | Self::BodyTransport | Self::Deadline => {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The lyric provider is unavailable",
                )
            }
            _ => (
                StatusCode::BAD_GATEWAY,
                "The lyric provider returned invalid data",
            ),
        }
    }
}

pub(crate) enum Error {
    InvalidRequest,
    InvalidResponse,
    Busy,
    NotFound,
    Upstream(UpstreamFailure),
}

impl Error {
    pub(crate) fn observe(self, operation: &'static str, started: Instant) -> Self {
        let diagnostic = match &self {
            Self::Upstream(failure) => Some(failure.diagnostic()),
            Self::InvalidResponse => Some(("invalid_record", "validation", None)),
            _ => None,
        };
        if let Some((reason, stage, upstream_status)) = diagnostic {
            let recovery = matches!(
                self,
                Self::Upstream(
                    UpstreamFailure::InvalidRetryAfter(_) | UpstreamFailure::RetryAfterOverflow(_)
                )
            )
            .then_some("restart_service");
            tracing::warn!(
                event = if recovery.is_some() {
                    "lrclib_upstream_cooldown_blocked"
                } else {
                    "lrclib_upstream_failed"
                },
                operation,
                failure_id = %uuid::Uuid::new_v4(),
                reason,
                stage,
                upstream_status,
                recovery,
                elapsed_ms = started.elapsed().as_millis() as u64,
            );
        }
        self
    }
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
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "The lyric provider record was not found",
            ),
            Self::Upstream(failure) => failure.response(),
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}
