use crate::InitError;
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use tracing::Instrument;
use zeroize::Zeroizing;

pub(crate) struct Policy {
    digest: [u8; 32],
    slots: Arc<Semaphore>,
}

impl Policy {
    pub(crate) fn new(token: String) -> Result<Self, InitError> {
        let token = Zeroizing::new(token);
        if !valid_token(&token) {
            return Err(InitError::InvalidServiceCredential);
        }
        Ok(Self {
            digest: Sha256::digest(token.as_bytes()).into(),
            slots: Arc::new(Semaphore::new(16)),
        })
    }

    fn authenticate(&self, headers: &HeaderMap) -> Result<(), Error> {
        let mut values = headers.get_all("authorization").iter();
        let token = values
            .next()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .filter(|v| valid_token(v))
            .ok_or(Error::Unauthorized)?;
        if values.next().is_some() {
            return Err(Error::Unauthorized);
        }
        let supplied: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        if !bool::from(supplied.ct_eq(&self.digest)) {
            return Err(Error::Unauthorized);
        }
        let mut versions = headers.get_all("x-hito-plugin-protocol").iter();
        if versions.next().is_none_or(|v| v != "1") || versions.next().is_some() {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }
}

fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.len() <= 4096 && token.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

pub(crate) async fn observe(
    State(policy): State<Arc<Policy>>,
    request: Request,
    next: Next,
) -> Response {
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/search" => "search",
        "/v1/recordings/detail" => "detail",
        "/v1/sources" => "sources",
        "/v1/sources/prepare" => "prepare",
        "/v1/sources/preparation/status" => "preparation_status",
        "/v1/audio" => "audio",
        _ => "unknown",
    };
    let span = tracing::debug_span!("torbox_plugin_request", request_id = %uuid::Uuid::new_v4(), operation);
    let mut log = RequestLog {
        dispatch: tracing::dispatcher::get_default(Clone::clone),
        span: span.clone(),
        started: Instant::now(),
        outcome: "cancelled",
        status: 0,
    };
    let result = async {
        if request.uri().query().is_some_and(|s| s.len() > 4096)
            || request.headers().len() > 32
            || request
                .headers()
                .iter()
                .map(|(k, v)| k.as_str().len() + v.as_bytes().len())
                .sum::<usize>()
                > 8192
        {
            return Err(Error::InvalidRequest);
        }
        if operation != "manifest" {
            policy.authenticate(request.headers())?;
        }
        let _permit = policy
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        tokio::time::timeout(Duration::from_secs(14), next.run(request))
            .await
            .map_err(|_| Error::Deadline)
    }
    .instrument(span)
    .await;
    let mut response = result.unwrap_or_else(IntoResponse::into_response);
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    log.status = response.status().as_u16();
    log.outcome = match log.status {
        200..=299 => "success",
        401 => "unauthorized",
        _ => "rejected",
    };
    response
}

struct RequestLog {
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    started: Instant,
    outcome: &'static str,
    status: u16,
}

impl Drop for RequestLog {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            event = "torbox_plugin_request_finished",
            outcome = self.outcome,
            status = self.status,
            elapsed_ms = self.started.elapsed().as_millis() as u64
        );
    }
}

pub(crate) enum Error {
    InvalidRequest,
    Unauthorized,
    NotFound,
    Conflict,
    Unavailable,
    Busy,
    Deadline,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "The request is invalid"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Plugin authorization is required"),
            Self::NotFound => (StatusCode::NOT_FOUND, "The requested source is unavailable"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "The operation belongs to another source",
            ),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "The source is unavailable"),
            Self::Busy => (StatusCode::TOO_MANY_REQUESTS, "The service is busy"),
            Self::Deadline => (
                StatusCode::GATEWAY_TIMEOUT,
                "The request exceeded its deadline",
            ),
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}
