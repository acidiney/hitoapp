use crate::{
    Config,
    error::{ApiError, ApiResult},
};
use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use hito_core::{AuthSession, Error};

pub fn token(headers: &HeaderMap) -> ApiResult<&str> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        return value
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|value| value.len() == 64)
            .ok_or(ApiError(Error::Unauthorized));
    }
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(';')
                .filter_map(|value| value.trim().split_once('='))
                .find(|(key, _)| *key == "hito_session")
                .map(|(_, value)| value)
        })
        .filter(|value| value.len() == 64)
        .ok_or(ApiError(Error::Unauthorized))
}

pub fn session_response(config: &Config, session: AuthSession, status: StatusCode) -> Response {
    let cookie = cookie(config, &session.token, 2592000);
    (status, [(header::SET_COOKIE, cookie)], Json(session)).into_response()
}

pub fn cookie(config: &Config, token: &str, max_age: u32) -> String {
    let secure = if config.origin.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    format!("hito_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}")
}
