mod body;
mod origin;
mod rate_limit;
mod session;

pub use session::{cookie, session_response, token};

use crate::{Config, error};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::Response,
};
use rate_limit::AuthLimiter;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

#[derive(Clone)]
pub struct Security {
    config: Config,
    attempts: Arc<AuthLimiter>,
    pin_attempts: Arc<AuthLimiter>,
    auth_slots: Arc<tokio::sync::Semaphore>,
}

impl Security {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            attempts: Arc::default(),
            pin_attempts: Arc::default(),
            auth_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}

pub async fn browser_security(
    State(security): State<Security>,
    mut request: Request,
    next: Next,
) -> Response {
    if !origin::allowed(
        &request,
        &security.config.origin,
        &security.config.additional_origins,
    ) {
        tracing::warn!(
            event = "origin_rejected",
            "Request origin could not be verified"
        );
        return error::response(
            StatusCode::FORBIDDEN,
            "Request origin could not be verified",
        );
    }
    let path = request.uri().path();
    let limiter =
        if request.method() == "POST" && ["/api/v1/setup", "/api/v1/login"].contains(&path) {
            Some(&security.attempts)
        } else if ["PUT", "POST"].contains(&request.method().as_str())
            && [
                "/api/v1/account/pin",
                "/api/v1/account/pin/remove",
                "/api/v1/account/unlock",
            ]
            .contains(&path)
        {
            Some(&security.pin_attempts)
        } else {
            None
        };
    let _permit = if let Some(limiter) = limiter {
        let ip = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|value| value.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        if !limiter.allow(ip) {
            tracing::warn!(event = "auth_rate_limited", "Sign-in rate limit exceeded");
            return rate_limit::response();
        }
        request = match body::buffer(request).await {
            Ok(request) => request,
            Err((status, message)) => return error::response(status, message),
        };
        match security.auth_slots.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                tracing::warn!(
                    event = "auth_capacity_exhausted",
                    "Sign-in capacity exhausted"
                );
                return rate_limit::response();
            }
        }
    } else {
        None
    };
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "same-origin".parse().unwrap());
    headers.insert("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'".parse().unwrap());
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert("private, no-store".parse().unwrap());
    response
}
