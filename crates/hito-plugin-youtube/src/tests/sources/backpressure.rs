use super::{Capture, bounded_body, exchange, https, owned_aac, private, representation};
use crate::{Config, Service};
use axum::http::{Method, StatusCode};
use hito_plugin_sdk::{RecordingRequest, SourcePage};
use http_body_util::BodyExt;
use serde_json::json;
use std::{
    fs,
    future::{Future, poll_fn},
    os::unix::fs::PermissionsExt,
    task::Poll,
    time::Duration,
};

#[tokio::test(flavor = "current_thread")]
async fn unpolled_audio_body_survives_a_consumer_pause_longer_than_the_upstream_idle_limit() {
    let directory = tempfile::tempdir().unwrap();
    let original = owned_aac(directory.path());
    let mut cdn = https::Fixture::start(original.clone()).await;
    let executable = directory.path().join("owned-recording-extractor");
    fs::write(&executable, include_str!("../../fixtures/recording.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        directory.path().join("recording.json"),
        json!({
            "id":"HitoTone001", "title":"Owned AAC tone", "artist":"Hito test performer",
            "duration":0.25, "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2",
            "vcodec":"none", "protocol":"https", "filesize":original.len(), "url":cdn.url(),
            "is_live":false, "live_status":"not_live"
        })
        .to_string(),
    )
    .unwrap();
    let dispatch = Capture::default().dispatch();
    let service = Service::with_media_client(Config { executable }, cdn.client());
    let app = service.router();
    let payload = serde_json::to_vec(&RecordingRequest {
        recording_id: "HitoTone001".into(),
    })
    .unwrap();
    let response = exchange(&app, &dispatch, Method::POST, "/v1/sources", None, &payload).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = bounded_body(response).await;
    private(&bytes, directory.path());
    let page: SourcePage = serde_json::from_slice(&bytes).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    let mut url = reqwest::Url::parse("http://owned.invalid/v1/audio").unwrap();
    url.query_pairs_mut()
        .append_pair("source_id", &page.items[0].id);
    let audio = format!("{}?{}", url.path(), url.query().unwrap());
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    representation(&response, StatusCode::OK, original.len(), None);

    tokio::time::pause();
    let paused = tokio::time::Instant::now();
    tokio::time::advance(Duration::from_secs(11)).await;
    assert_eq!(paused.elapsed(), Duration::from_secs(11));
    tokio::time::resume();

    assert_eq!(bounded_body(response).await.as_ref(), original.as_slice());
    tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .await
        .unwrap()
        .unwrap();
    cdn.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn actively_waiting_audio_times_out_closes_upstream_and_releases_its_relay_permit() {
    let directory = tempfile::tempdir().unwrap();
    let original = owned_aac(directory.path());
    let mut cdn = https::Fixture::start(original.clone()).await;
    let executable = directory.path().join("owned-recording-extractor");
    fs::write(&executable, include_str!("../../fixtures/recording.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        directory.path().join("recording.json"),
        json!({
            "id":"HitoTone001", "title":"Owned AAC tone", "artist":"Hito test performer",
            "duration":0.25, "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2",
            "vcodec":"none", "protocol":"https", "filesize":original.len(), "url":cdn.url(),
            "is_live":false, "live_status":"not_live"
        })
        .to_string(),
    )
    .unwrap();
    let logs = Capture::default();
    let dispatch = logs.dispatch();
    let service = Service::with_media_client(Config { executable }, cdn.client());
    let app = service.router();
    let payload = serde_json::to_vec(&RecordingRequest {
        recording_id: "HitoTone001".into(),
    })
    .unwrap();
    let response = exchange(&app, &dispatch, Method::POST, "/v1/sources", None, &payload).await;
    assert_eq!(response.status(), StatusCode::OK);
    let page: SourcePage = serde_json::from_slice(&bounded_body(response).await).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    let mut url = reqwest::Url::parse("http://owned.invalid/v1/audio").unwrap();
    url.query_pairs_mut()
        .append_pair("source_id", &page.items[0].id);
    let audio = format!("{}?{}", url.path(), url.query().unwrap());

    cdn.hold_until_closed(4);
    let mut held = Vec::new();
    for _ in 0..3 {
        let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
        representation(&response, StatusCode::OK, original.len(), None);
        held.push(response.into_body());
    }
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    representation(&response, StatusCode::OK, original.len(), None);
    let mut body = response.into_body();
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let mut prefix = Vec::new();
    while prefix.len() < 64 {
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert!(frame.len() <= 64 - prefix.len());
        prefix.extend_from_slice(&frame);
    }
    assert_eq!(prefix.as_slice(), &original[..64]);
    let mut waiting = Box::pin(body.frame());
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;

    tokio::time::pause();
    let paused = tokio::time::Instant::now();
    tokio::time::advance(Duration::from_secs(11)).await;
    assert_eq!(paused.elapsed(), Duration::from_secs(11));
    tokio::time::resume();

    let error = tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .expect("The active upstream idle deadline did not terminate the body")
        .expect("An unfinished representation ended without a deadline error")
        .expect_err("The held upstream unexpectedly returned more audio");
    assert_eq!(error.to_string(), "YouTube audio input stopped");
    assert!(body.frame().await.is_none());
    cdn.closed(1).await;
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    representation(&response, StatusCode::OK, original.len(), None);
    assert_eq!(bounded_body(response).await.as_ref(), original.as_slice());
    drop(held);
    cdn.closed(3).await;
    tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .await
        .unwrap()
        .unwrap();
    cdn.shutdown().await;
    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(output.lines().any(|line| {
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        event["fields"]["event"] == "youtube_audio_complete"
            && event["fields"]["outcome"] == "deadline"
    }));
}
