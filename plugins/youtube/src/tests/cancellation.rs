use super::lifecycle::{HeldReleaseOnDrop, ProcessIdentity};
use crate::{Config, Service};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use hito_plugin_sdk::SearchPage;
use http_body_util::BodyExt;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canceling_a_polled_request_cleans_its_group_and_returns_owned_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let _cleanup = HeldReleaseOnDrop(directory.path().to_owned());
    let executable = directory.path().join("private-executable-path-sentinel");
    fs::write(&executable, include_str!("../fixtures/cancellation.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::new(Config { executable });
    let first = tokio::spawn(
        service
            .router()
            .oneshot(request("private-first-query-sentinel")),
    );
    let second = tokio::spawn(
        service
            .router()
            .oneshot(request("private-second-query-sentinel")),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while !["first", "second"]
        .iter()
        .all(|slot| directory.path().join(slot).join("leader.ready").exists())
    {
        assert!(
            Instant::now() < deadline,
            "Both owned groups must become ready"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (first_leader, first_child) = group(&directory.path().join("first"));
    let (second_leader, second_child) = group(&directory.path().join("second"));
    assert!(!first.is_finished());
    assert!(!second.is_finished());
    assert_eq!(calls(directory.path()), 2);

    let response = tokio::time::timeout(
        Duration::from_secs(1),
        service
            .router()
            .oneshot(request("private-third-query-sentinel")),
    )
    .await
    .unwrap()
    .unwrap();
    busy(response, directory.path()).await;
    assert_eq!(
        calls(directory.path()),
        2,
        "A full service invoked a third executable"
    );

    first.abort();
    assert!(matches!(first.await, Err(error) if error.is_cancelled()));
    let deadline = Instant::now() + Duration::from_secs(3);
    while first_leader.present().unwrap() || first_child.running().unwrap() {
        assert!(
            Instant::now() < deadline,
            "Canceled HTTP work retained its owned group"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(second_leader.running().unwrap());
    assert!(second_child.running().unwrap());

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut attempts = 0;
    let response = loop {
        assert!(
            attempts < 100 && Instant::now() < deadline,
            "Canceled work did not return its search permit"
        );
        attempts += 1;
        assert!(second_leader.running().unwrap());
        assert!(second_child.running().unwrap());
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            service
                .router()
                .oneshot(request("private-fourth-query-sentinel")),
        )
        .await
        .unwrap()
        .unwrap();
        if response.status() == StatusCode::OK {
            break response;
        }
        busy(response, directory.path()).await;
        assert_eq!(calls(directory.path()), 2);
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    private_response(&bytes, directory.path());
    let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
    page.validate(1).unwrap();
    assert_eq!(page.items[0].id, "HitoTone004");
    assert_eq!(calls(directory.path()), 3);
    assert!(second_leader.running().unwrap());
    assert!(second_child.running().unwrap());

    tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(!second_leader.present().unwrap());
    assert!(!second_child.running().unwrap());
    let response = tokio::time::timeout(Duration::from_secs(1), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    private_response(&bytes, directory.path());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        serde_json::json!({"error":"The YouTube service is unavailable"})
    );
    assert_eq!(calls(directory.path()), 3);
}

fn request(query: &str) -> Request<Body> {
    Request::post("/v1/search")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"query":query,"cursor":null,"limit":1}).to_string(),
        ))
        .unwrap()
}

fn group(directory: &Path) -> (ProcessIdentity, ProcessIdentity) {
    let leader = ProcessIdentity::read(&directory.join("leader.identity")).unwrap();
    let child = ProcessIdentity::read(&directory.join("child.identity")).unwrap();
    assert_eq!(leader.group, leader.pid);
    assert_eq!(child.group, leader.pid);
    assert!(leader.running().unwrap());
    assert!(child.running().unwrap());
    (leader, child)
}

fn calls(directory: &Path) -> usize {
    fs::read_to_string(directory.join("calls"))
        .unwrap()
        .lines()
        .count()
}

async fn busy(response: Response, directory: &Path) {
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    private_response(&bytes, directory);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        serde_json::json!({"error":"The YouTube service is busy. Try again later"})
    );
}

fn private_response(bytes: &[u8], directory: &Path) {
    let body = std::str::from_utf8(bytes).unwrap();
    assert!(
        !body.contains("private-"),
        "Response exposed injected private content"
    );
    assert!(
        !body.contains(directory.to_str().unwrap()),
        "Response exposed an owned private path"
    );
}
