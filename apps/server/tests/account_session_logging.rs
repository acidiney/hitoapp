use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{CoreConfig, HostDeviceId};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-hito-request", "1");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn password_and_session_workflows_log_outcomes_without_secrets() {
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    let dir = tempfile::tempdir().unwrap();
    let secrets = std::cell::RefCell::new(Vec::new());
    async {
        let services = hito_core::open(CoreConfig::new(
            dir.path().join("hito.db"),
            dir.path().join("music"),
            HostDeviceId::new("session-logging-host").unwrap(),
        ))
        .await
        .unwrap();
        let app = hito_server::router(services);
        let first = send(
            &app,
            "POST",
            "/api/v1/setup",
            None,
            json!({"username":"admin","password":"first-password-sentinel"}),
        )
        .await
        .1["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let second = send(
            &app,
            "POST",
            "/api/v1/login",
            None,
            json!({"username":"admin","password":"first-password-sentinel"}),
        )
        .await
        .1["token"]
            .as_str()
            .unwrap()
            .to_owned();
        secrets.borrow_mut().extend([first.clone(), second.clone()]);
        let wrong = send(
            &app,
            "POST",
            "/api/v1/account/password",
            Some(&first),
            json!({"current_password":"wrong-password-sentinel","new_password":"next-password-sentinel"}),
        )
        .await;
        assert_eq!(wrong.0, StatusCode::UNPROCESSABLE_ENTITY);
        let changed = send(
            &app,
            "POST",
            "/api/v1/account/password",
            Some(&first),
            json!({"current_password":"first-password-sentinel","new_password":"next-password-sentinel"}),
        )
        .await;
        assert_eq!(changed.0, StatusCode::OK);
        let next_login = || async {
            send(
                &app,
                "POST",
                "/api/v1/login",
                None,
                json!({"username":"admin","password":"next-password-sentinel"}),
            )
            .await
            .1["token"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        secrets.borrow_mut().push(next_login().await);
        let (_, list) = send(&app, "GET", "/api/v1/account/sessions", Some(&first), Value::Null).await;
        let other = list
            .as_array()
            .unwrap()
            .iter()
            .find(|session| session["current"] == false)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        secrets.borrow_mut().push(other.clone());
        let revoked = send(&app, "DELETE", &format!("/api/v1/account/sessions/{other}"), Some(&first), Value::Null).await;
        assert_eq!(revoked.0, StatusCode::NO_CONTENT);
        secrets.borrow_mut().push(next_login().await);
        let others = send(&app, "DELETE", "/api/v1/account/sessions", Some(&first), Value::Null).await;
        assert_eq!(others.1, json!({"revoked": 1}));
        let (_, relist) = send(&app, "GET", "/api/v1/account/sessions", Some(&first), Value::Null).await;
        secrets.borrow_mut().push(relist[0]["id"].as_str().unwrap().to_owned());
        for _ in 0..5 {
            send(
                &app,
                "POST",
                "/api/v1/account/password",
                Some(&first),
                json!({"current_password":"wrong-password-sentinel","new_password":"last-password-sentinel"}),
            )
            .await;
        }
        let limited = send(
            &app,
            "POST",
            "/api/v1/account/password",
            Some(&first),
            json!({"current_password":"next-password-sentinel","new_password":"last-password-sentinel"}),
        )
        .await;
        assert_eq!(limited.0, StatusCode::TOO_MANY_REQUESTS);
    }
    .with_subscriber(subscriber)
    .await;
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    for event in [
        "password_confirmation_rejected",
        "password_changed",
        "session_revoked",
        "other_sessions_revoked",
    ] {
        assert!(text.contains(event), "{event}");
    }
    let secrets = secrets.into_inner();
    for sentinel in [
        "first-password-sentinel",
        "wrong-password-sentinel",
        "next-password-sentinel",
        "last-password-sentinel",
        "$argon2",
    ] {
        assert!(!text.contains(sentinel), "{sentinel}");
    }
    for secret in &secrets {
        assert!(!text.contains(secret.as_str()));
    }
}
