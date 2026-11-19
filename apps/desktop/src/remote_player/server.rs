use crate::remote::ServerSession;
use hito_core::{Error, Result};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use std::sync::Arc;
use url::Url;

pub struct Server {
    client: reqwest::Client,
    origin: Url,
    session: Arc<dyn ServerSession>,
    lease_tag: String,
}

#[derive(Deserialize)]
struct Problem {
    error: String,
}

impl Server {
    pub fn new(client: reqwest::Client, origin: Url, session: Arc<dyn ServerSession>) -> Self {
        Self {
            client,
            origin,
            session,
            lease_tag: format!("desktop-{}", uuid::Uuid::new_v4()),
        }
    }

    pub fn lease_tag(&self) -> &str {
        &self.lease_tag
    }

    pub fn token(&self) -> Option<String> {
        self.session.token()
    }

    pub fn request(&self, method: Method, segments: &[&str]) -> Result<RequestBuilder> {
        let token = self.session.token().ok_or(Error::Unauthorized)?;
        let mut url = self.origin.clone();
        url.path_segments_mut()
            .map_err(|_| Error::Upstream("The server address cannot take a path"))?
            .clear()
            .extend(["api", "v1"])
            .extend(segments);
        Ok(self.client.request(method, url).bearer_auth(token))
    }

    pub async fn send(&self, request: RequestBuilder) -> Result<Response> {
        let response = request
            .send()
            .await
            .map_err(|_| Error::Upstream("The server could not be reached"))?;
        if response.status().is_success() {
            return Ok(response);
        }
        Err(failure(response).await)
    }

    pub async fn json<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        self.send(request)
            .await?
            .json()
            .await
            .map_err(|_| Error::Upstream("The server sent an answer this app cannot read"))
    }
}

pub async fn failure(response: Response) -> Error {
    let status = response.status();
    let retry = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let message = response
        .json::<Problem>()
        .await
        .map(|problem| problem.error)
        .unwrap_or_default();
    status_error(status, message, retry)
}

pub fn status_error(status: StatusCode, message: String, retry: u64) -> Error {
    match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized,
        StatusCode::FORBIDDEN => Error::Forbidden,
        StatusCode::NOT_FOUND => Error::NotFound,
        StatusCode::LOCKED => Error::Locked,
        StatusCode::CONFLICT => Error::Conflict(message),
        StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST => Error::Invalid(message),
        StatusCode::TOO_MANY_REQUESTS => Error::RateLimited(retry),
        _ => Error::Upstream("The server could not finish the request"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_answers_keep_their_meaning_on_this_computer() {
        let cases = [
            (StatusCode::UNAUTHORIZED, "unauthorized"),
            (StatusCode::FORBIDDEN, "forbidden"),
            (StatusCode::NOT_FOUND, "not_found"),
            (StatusCode::LOCKED, "locked"),
            (StatusCode::CONFLICT, "conflict"),
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid"),
            (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            (StatusCode::BAD_GATEWAY, "upstream"),
        ];
        for (status, kind) in cases {
            let error = status_error(status, "reason".into(), 3);
            assert_eq!(error.code(), kind, "{status}");
        }
    }
}
