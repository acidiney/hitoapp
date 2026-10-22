use hito_testkit::plugin_server;
use reqwest::{Method, StatusCode};
use serde_json::Value;

pub use plugin_server::Server;

pub async fn json(
    client: &reqwest::Client,
    server: &Server,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    plugin_server::json(client, server, method, path, super::COOKIE, token, body).await
}
