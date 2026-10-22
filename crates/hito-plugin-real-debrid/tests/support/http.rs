use axum::body::to_bytes;
use reqwest::{Method, StatusCode};
use serde_json::Value;

pub use hito_testkit::plugin_server::Server;

pub async fn json(
    client: &reqwest::Client,
    server: &Server,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    hito_testkit::plugin_server::json(client, server, method, path, super::COOKIE, token, body)
        .await
}

pub async fn capture_request(
    request: axum::extract::Request,
) -> (axum::http::request::Parts, Vec<u8>) {
    let (parts, body) = request.into_parts();
    assert!(
        parts.uri.to_string().len() <= 4096,
        "owned request target exceeded its bound"
    );
    assert!(
        parts.headers.len() <= 32,
        "owned request header count exceeded its bound"
    );
    assert!(
        parts
            .headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>()
            <= 8192,
        "owned request headers exceeded their byte bound"
    );
    let body = to_bytes(body, 4096)
        .await
        .expect("owned request body must remain bounded");
    (parts, body.to_vec())
}
