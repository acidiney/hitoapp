use crate::support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

pub struct Account {
    token: String,
    pub id: String,
}

impl Account {
    pub async fn setup(app: &Router) -> Self {
        let (status, session) = support::call(
            app,
            "POST",
            "/api/v1/setup",
            json!({"username": "owner", "password": "owned artist review administrator password"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        Self {
            token: session["token"].as_str().unwrap().into(),
            id: session["user"]["id"].as_str().unwrap().into(),
        }
    }

    pub async fn request(
        &self,
        app: &Router,
        method: &str,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let (status, _, value) = exchange(app, method, path, body, self.headers()).await;
        (status, value)
    }

    pub fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            format!("hito_session={}", self.token).parse().unwrap(),
        );
        headers.insert("x-hito-account", self.id.parse().unwrap());
        headers.insert("x-hito-request", "1".parse().unwrap());
        headers.insert("origin", "http://localhost:8080".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        headers
    }

    pub async fn login(app: &Router, username: &str, password: &str) -> Self {
        let (status, session) = support::call(
            app,
            "POST",
            "/api/v1/login",
            json!({"username": username, "password": password}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        Self {
            token: session["token"].as_str().unwrap().into(),
            id: session["user"]["id"].as_str().unwrap().into(),
        }
    }

    pub async fn get(&self, app: &Router, path: &str) -> Value {
        let (status, value) = self.request(app, "GET", path, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        value
    }

    pub async fn connect(
        &self,
        app: &Router,
        address: &str,
        name: &str,
        token: Option<&str>,
    ) -> Value {
        let (status, service) = self
            .request(
                app,
                "POST",
                "/api/v1/plugins/services",
                json!({"name": name, "base_url": address}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, installation) = self
            .request(
                app,
                "POST",
                "/api/v1/plugins",
                json!({
                    "service_id": service["id"], "service_revision": service["revision"],
                    "bearer_token": token,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        installation
    }
}

pub async fn exchange(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, Value) {
    let body = if body.is_null() {
        Body::empty()
    } else {
        let bytes = serde_json::to_vec(&body).unwrap();
        assert!(bytes.len() <= 262_144);
        Body::from(bytes)
    };
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .body(body)
        .unwrap();
    *request.headers_mut() = headers;
    let response = tokio::time::timeout(Duration::from_secs(5), app.clone().oneshot(request))
        .await
        .expect("The owned HTTP request must finish within its deadline")
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 524_288).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, headers, value)
}
