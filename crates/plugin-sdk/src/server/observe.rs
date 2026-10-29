use super::{AuthFailure, ServiceAuth};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tracing::Instrument;

const MAX_QUERY_BYTES: usize = 4096;
const MAX_HEADER_COUNT: usize = 32;
const MAX_HEADER_BYTES: usize = 8192;
const CONCURRENT_REQUESTS: usize = 16;
const REQUEST_DEADLINE: Duration = Duration::from_secs(14);

/// Failures the request layer itself produces, with fixed public messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    InvalidRequest,
    Unauthorized,
    Busy,
    Deadline,
}

impl From<AuthFailure> for Rejection {
    fn from(failure: AuthFailure) -> Self {
        match failure {
            AuthFailure::Unauthorized => Self::Unauthorized,
            AuthFailure::InvalidProtocol => Self::InvalidRequest,
        }
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The request is invalid"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Plugin authorization is required"),
            Self::Busy => (StatusCode::TOO_MANY_REQUESTS, "The service is busy"),
            Self::Deadline => (
                StatusCode::GATEWAY_TIMEOUT,
                "The request exceeded its deadline",
            ),
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}

pub struct Finished {
    pub outcome: &'static str,
    pub status: u16,
    pub elapsed_ms: u64,
}

/// Plugin-named span and completion event, built with `request_telemetry!`.
pub struct Telemetry {
    pub span: fn(uuid::Uuid, &'static str) -> tracing::Span,
    pub finished: fn(&Finished),
}

pub struct RequestPolicy {
    telemetry: Telemetry,
    auth: Option<ServiceAuth>,
    request_id_header: bool,
    slots: Arc<Semaphore>,
}

impl RequestPolicy {
    /// With `auth` set, every route except the manifest needs the service bearer.
    pub fn new(telemetry: Telemetry, auth: Option<ServiceAuth>) -> Self {
        Self {
            telemetry,
            auth,
            request_id_header: false,
            slots: Arc::new(Semaphore::new(CONCURRENT_REQUESTS)),
        }
    }

    pub fn with_request_id_header(mut self) -> Self {
        self.request_id_header = true;
        self
    }
}

pub async fn observe_requests(
    State(policy): State<Arc<RequestPolicy>>,
    request: Request,
    next: Next,
) -> Response {
    let operation = operation(request.uri().path());
    let request_id = uuid::Uuid::new_v4();
    let span = (policy.telemetry.span)(request_id, operation);
    let mut log = RequestLog {
        dispatch: tracing::dispatcher::get_default(Clone::clone),
        span: span.clone(),
        started: Instant::now(),
        finished: policy.telemetry.finished,
        outcome: "cancelled",
        status: 0,
    };
    let result = async {
        if request
            .uri()
            .query()
            .is_some_and(|value| value.len() > MAX_QUERY_BYTES)
            || request.headers().len() > MAX_HEADER_COUNT
            || request
                .headers()
                .iter()
                .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
                .sum::<usize>()
                > MAX_HEADER_BYTES
        {
            return Err(Rejection::InvalidRequest);
        }
        if operation != "manifest"
            && let Some(auth) = &policy.auth
        {
            auth.check(request.headers())?;
        }
        let _permit = policy
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Rejection::Busy)?;
        tokio::time::timeout(REQUEST_DEADLINE, next.run(request))
            .await
            .map_err(|_| Rejection::Deadline)
    }
    .instrument(span)
    .await;
    let mut response = result.unwrap_or_else(IntoResponse::into_response);
    let headers = response.headers_mut();
    headers.insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    if policy.request_id_header {
        headers.insert(
            "x-request-id",
            HeaderValue::from_str(&request_id.to_string())
                .expect("a generated request UUID is a valid header value"),
        );
    }
    log.status = response.status().as_u16();
    log.outcome = match log.status {
        200..=299 => "success",
        401 => "unauthorized",
        _ => "rejected",
    };
    response
}

fn operation(path: &str) -> &'static str {
    match path {
        "/v1/manifest" => "manifest",
        "/v1/search" => "search",
        "/v1/recordings/detail" => "detail",
        "/v1/sources" => "sources",
        "/v1/sources/prepare" => "prepare",
        "/v1/sources/preparation/status" => "preparation_status",
        "/v1/audio" => "audio",
        _ => "unknown",
    }
}

struct RequestLog {
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    started: Instant,
    finished: fn(&Finished),
    outcome: &'static str,
    status: u16,
}

impl Drop for RequestLog {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        (self.finished)(&Finished {
            outcome: self.outcome,
            status: self.status,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
        });
    }
}
