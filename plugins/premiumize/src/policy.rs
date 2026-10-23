use crate::{config::Connection, http::HttpState};
use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub(crate) struct AuthorizedConnection(pub(crate) Arc<Connection>);

impl FromRequestParts<Arc<HttpState>> for AuthorizedConnection {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<HttpState>) -> Result<Self, Error> {
        let mut values = parts.headers.get_all("authorization").iter();
        let supplied = values
            .next()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(credential_digest)
            .ok_or(Error::Unauthorized)?;
        if values.next().is_some() {
            return Err(Error::Unauthorized);
        }
        let mut selected = None;
        for connection in &state.connections {
            if bool::from(supplied.ct_eq(&connection.service_authority)) {
                selected = Some(connection.clone());
            }
        }
        let connection = selected.ok_or(Error::Unauthorized)?;
        let mut versions = parts.headers.get_all("x-hito-plugin-protocol").iter();
        if versions.next().is_none_or(|value| value != "1") || versions.next().is_some() {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(connection))
    }
}

pub(crate) fn credential_digest(value: &str) -> Option<[u8; 32]> {
    if value.is_empty()
        || value.len() > 4096
        || !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return None;
    }
    Some(Sha256::digest(value.as_bytes()).into())
}

pub(crate) enum Error {
    InvalidRequest,
    Unauthorized,
    NotFound,
    MethodNotAllowed,
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
            Self::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "The method is not supported",
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
