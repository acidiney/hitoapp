use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use hito_server::{HostChoice, HostFolders};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;

struct Fake {
    suggestion: Option<PathBuf>,
    chosen: Mutex<Option<PathBuf>>,
    busy: bool,
}

impl HostFolders for Fake {
    fn suggestion(&self) -> BoxFuture<'_, Option<PathBuf>> {
        Box::pin(async { self.suggestion.clone() })
    }
    fn choose(&self) -> BoxFuture<'_, HostChoice> {
        Box::pin(async {
            match self.chosen.lock().unwrap().take() {
                Some(path) => HostChoice::Chosen(path),
                None if self.busy => HostChoice::Busy,
                None => HostChoice::Cancelled,
            }
        })
    }
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
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

async fn setup(host: Option<Arc<Fake>>) -> (tempfile::TempDir, PathBuf, Router, String) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("PRIVATE_MUSIC");
    std::fs::create_dir_all(&music).unwrap();
    let services = hito_core::open(hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music.clone(),
        hito_core::HostDeviceId::new("host-folders").unwrap(),
    ))
    .await
    .unwrap();
    let config = hito_server::Config::default();
    let app = match host {
        Some(host) => hito_server::router_with_host(services, config, None, host),
        None => hito_server::router_with_config(services, config),
    };
    let token = call(
        &app,
        "POST",
        "/api/v1/setup",
        json!({"username":"admin","password":"administrator test password"}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, music, app, token)
}

#[tokio::test]
async fn browser_hosts_offer_no_picker_and_reject_choosing() {
    let (_dir, _music, app, token) = setup(None).await;
    let (status, offer) = call(
        &app,
        "GET",
        "/api/v1/host/folders",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(offer, json!({"picker": false, "suggestion": null}));
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/host/folders/choose",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn desktop_hosts_suggest_a_granted_folder_and_grant_a_chosen_one() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("PRIVATE_ELSEWHERE");
    std::fs::create_dir_all(&outside).unwrap();
    let host = Arc::new(Fake {
        suggestion: None,
        chosen: Mutex::new(Some(outside)),
        busy: false,
    });
    let (_dir, _music, app, token) = setup(Some(host)).await;
    let (_, offer) = call(
        &app,
        "GET",
        "/api/v1/host/folders",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(offer, json!({"picker": true, "suggestion": null}));
    let (status, chosen) = call(
        &app,
        "POST",
        "/api/v1/host/folders/choose",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let directory = chosen["directory"].clone();
    assert_eq!(directory["components"], json!([]));
    assert!(!chosen.to_string().contains("PRIVATE_ELSEWHERE"));
    let (_, shares) = call(
        &app,
        "GET",
        "/api/v1/storage/directories",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(shares.as_array().unwrap().len(), 2);
    assert!(shares.to_string().contains("PRIVATE_ELSEWHERE"));
    assert!(!shares.to_string().contains(dir.path().to_str().unwrap()));
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/folders"),
        json!({"directory": directory}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn cancelling_the_picker_changes_nothing_and_a_suggestion_never_grants() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let host = Arc::new(Fake {
        suggestion: Some(elsewhere),
        chosen: Mutex::new(None),
        busy: false,
    });
    let (_dir, _music, app, token) = setup(Some(host)).await;
    let (_, offer) = call(
        &app,
        "GET",
        "/api/v1/host/folders",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(offer, json!({"picker": true, "suggestion": null}));
    let (status, chosen) = call(
        &app,
        "POST",
        "/api/v1/host/folders/choose",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(chosen, json!({"directory": null}));
    let (_, shares) = call(
        &app,
        "GET",
        "/api/v1/storage/directories",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(shares.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_suggestion_inside_a_grant_is_offered_as_a_reference() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music-root");
    std::fs::create_dir_all(&music).unwrap();
    let services = hito_core::open(hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music.clone(),
        hito_core::HostDeviceId::new("host-folders").unwrap(),
    ))
    .await
    .unwrap();
    let host = Arc::new(Fake {
        suggestion: Some(music),
        chosen: Mutex::new(None),
        busy: false,
    });
    let app = hito_server::router_with_host(services, hito_server::Config::default(), None, host);
    let token = call(
        &app,
        "POST",
        "/api/v1/setup",
        json!({"username":"admin","password":"administrator test password"}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, offer) = call(
        &app,
        "GET",
        "/api/v1/host/folders",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(
        offer["suggestion"],
        json!({"directory_id":"legacy-music","components":[]})
    );
}

#[tokio::test]
async fn listeners_and_anonymous_callers_are_refused() {
    let host = Arc::new(Fake {
        suggestion: None,
        chosen: Mutex::new(None),
        busy: false,
    });
    let (_dir, _music, app, admin) = setup(Some(host)).await;
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"listener","password":"listener folder password"}),
        Some(&admin),
    )
    .await;
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"listener","password":"listener folder password"}),
        None,
    )
    .await;
    let listener = session["token"].as_str().unwrap();
    for (method, path) in [
        ("GET", "/api/v1/host/folders"),
        ("POST", "/api/v1/host/folders/choose"),
    ] {
        assert_eq!(
            call(&app, method, path, Value::Null, None).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, method, path, Value::Null, Some(listener))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn a_second_chooser_reports_busy_as_a_conflict() {
    let host = Arc::new(Fake {
        suggestion: None,
        chosen: Mutex::new(None),
        busy: true,
    });
    let (_dir, _music, app, token) = setup(Some(host)).await;
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/host/folders/choose",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.to_string().contains("already open"));
}

#[tokio::test]
async fn choosing_requires_the_request_marker_header() {
    let host = Arc::new(Fake {
        suggestion: None,
        chosen: Mutex::new(None),
        busy: false,
    });
    let (_dir, _music, app, token) = setup(Some(host)).await;
    let post = |marker: bool, origin: Option<&str>| {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/host/folders/choose")
            .header("cookie", format!("hito_session={token}"));
        if marker {
            request = request.header("x-hito-request", "1");
        }
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        app.clone().oneshot(request.body(Body::empty()).unwrap())
    };
    assert_eq!(
        post(false, None).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(true, Some("https://evil.invalid"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(post(true, None).await.unwrap().status(), StatusCode::OK);
}
