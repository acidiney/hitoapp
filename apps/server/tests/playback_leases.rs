#[path = "playback_leases/contract.rs"]
mod contract;
#[path = "playback_leases/fixture.rs"]
mod fixture;
#[path = "playback_leases/input.rs"]
mod input;
#[path = "playback_leases/logs.rs"]
mod logs;
#[path = "playback_leases/pin.rs"]
mod pin;
#[path = "playback_leases/representation.rs"]
mod representation;
#[path = "playback_leases/runtime.rs"]
mod runtime;

use axum::http::{StatusCode, header};
use fixture::Case;
use hito_core::PlaybackLease;
use http_body_util::BodyExt;

#[tokio::test]
async fn an_authenticated_session_leases_original_audio_and_releases_the_handle() {
    let f = Case::new().await;
    let response = f.begin().await;
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    if status != StatusCode::OK {
        f.close().await;
        assert_eq!(
            status,
            StatusCode::OK,
            "begin must expose the Core lease capability"
        );
        return;
    }
    let lease: PlaybackLease = serde_json::from_slice(&bytes).unwrap();
    let path = format!("/api/v1/playback/leases/{}", lease.id);
    let response = f.request("GET", &path, &[], None).await;
    let status_ok = response.status() == StatusCode::OK;
    let audio = format!("{path}/audio");
    let head = f.request("HEAD", &audio, &[], None).await;
    let head_ok = head.status() == StatusCode::OK
        && head.headers()[header::CONTENT_LENGTH] == f.original.len().to_string()
        && head.headers()[header::CACHE_CONTROL] == "private, no-store"
        && !head.headers().contains_key(header::ETAG);
    let head_empty = head
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty();
    let range = f
        .request("GET", &audio, &[("range", "bytes=7-19")], None)
        .await;
    let range_ok = range.status() == StatusCode::PARTIAL_CONTENT
        && range.headers()[header::CONTENT_RANGE] == format!("bytes 7-19/{}", f.original.len())
        && range.headers()[header::CACHE_CONTROL] == "private, no-store";
    let range_bytes = range.into_body().collect().await.unwrap().to_bytes();
    let original = f.original.clone();
    let released = f.request("DELETE", &path, &[], None).await.status();
    let stopped = f.request("GET", &audio, &[], None).await.status();
    f.close().await;
    assert!(status_ok && head_ok && head_empty && range_ok);
    assert_eq!(range_bytes.as_ref(), &original[7..20]);
    assert_eq!(released, StatusCode::NO_CONTENT);
    assert_ne!(stopped, StatusCode::OK);
}
