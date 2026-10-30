use crate::{config::Connection, http::HttpState};
use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::server::{bearer_digest, require_protocol};
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub(crate) struct AuthorizedConnection(pub(crate) Arc<Connection>);

impl FromRequestParts<Arc<HttpState>> for AuthorizedConnection {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<HttpState>) -> Result<Self, Error> {
        let supplied = bearer_digest(&parts.headers).map_err(|_| Error::Unauthorized)?;
        let mut selected = None;
        for connection in &state.connections {
            if bool::from(supplied.ct_eq(&connection.service_authority)) {
                selected = Some(connection.clone());
            }
        }
        let connection = selected.ok_or(Error::Unauthorized)?;
        require_protocol(&parts.headers).map_err(|_| Error::InvalidRequest)?;
        Ok(Self(connection))
    }
}

pub(crate) enum Error {
    InvalidRequest,
    Unauthorized,
    NotFound,
    MethodNotAllowed,
    Unavailable,
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
        };
        (status, Json(serde_json::json!({"error":message}))).into_response()
    }
}
