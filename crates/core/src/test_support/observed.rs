use axum::http::{HeaderMap, Method};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedRequest {
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Value,
}

pub fn observed(plugin: &FakePlugin, limit: usize) -> Vec<ObservedRequest> {
    plugin
        .requests()
        .into_iter()
        .take(limit)
        .map(|request| ObservedRequest {
            path: format!("/prefix/{}", request.operation),
            body: request.json(),
            headers: request.headers,
            method: request.method,
        })
        .collect()
}
