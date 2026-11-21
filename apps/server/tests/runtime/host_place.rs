use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use hito_server::{HostPlace, HostPlaceCurrent, Hosts};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

#[derive(Default)]
struct Fake {
    changes: AtomicUsize,
    forgotten: AtomicUsize,
}

impl HostPlace for Fake {
    fn current(&self) -> HostPlaceCurrent {
        HostPlaceCurrent::ThisComputer
    }
    fn change(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {
            self.changes.fetch_add(1, Ordering::SeqCst);
        })
    }
    fn forget_me(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {
            self.forgotten.fetch_add(1, Ordering::SeqCst);
        })
    }
}

async fn call(app: &Router, method: &str, path: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-hito-request", "1");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = if method == "POST" { "{}" } else { "" };
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn setup(place: Option<Arc<Fake>>) -> (tempfile::TempDir, Router, String) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let services = hito_core::open(hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("host-place").unwrap(),
    ))
    .await
    .unwrap();
    let hosts = Hosts {
        place: place.map(|place| place as Arc<dyn HostPlace>),
        ..Hosts::default()
    };
    let app = hito_server::router_with_hosts(services, hito_server::Config::default(), None, hosts);
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/setup")
        .header("content-type", "application/json")
        .header("x-hito-request", "1")
        .body(Body::from(
            json!({"username":"owner","password":"place owner test password"}).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let token = serde_json::from_slice::<Value>(&bytes).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, app, token)
}

#[tokio::test]
async fn a_server_without_an_app_host_offers_no_place_change() {
    let (_dir, app, token) = setup(None).await;
    let (status, offer) = call(&app, "GET", "/api/v1/host/place", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(offer, json!({"changeable": false, "current": null}));
    let (status, _) = call(&app, "POST", "/api/v1/host/place/change", Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_desktop_app_reports_its_place_and_is_asked_to_change_it() {
    let place = Arc::new(Fake::default());
    let (_dir, app, token) = setup(Some(place.clone())).await;
    let (status, offer) = call(&app, "GET", "/api/v1/host/place", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        offer,
        json!({"changeable": true, "current": {"kind": "this_computer"}})
    );
    let (status, _) = call(&app, "POST", "/api/v1/host/place/change", Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(place.changes.load(Ordering::SeqCst), 1);
    let (status, _) = call(&app, "POST", "/api/v1/host/place/forget-me", Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(place.forgotten.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_place_shows_on_the_sign_in_page_but_changing_it_needs_an_account() {
    let place = Arc::new(Fake::default());
    let (_dir, app, _token) = setup(Some(place.clone())).await;
    let (status, offer) = call(&app, "GET", "/api/v1/host/place", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        offer,
        json!({"changeable": true, "current": {"kind": "this_computer"}})
    );
    let (status, _) = call(&app, "POST", "/api/v1/host/place/change", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "POST", "/api/v1/host/place/forget-me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(place.changes.load(Ordering::SeqCst), 0);
    assert_eq!(place.forgotten.load(Ordering::SeqCst), 0);
}
