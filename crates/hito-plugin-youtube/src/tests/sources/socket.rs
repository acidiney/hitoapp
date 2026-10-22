use super::{Capture, https, owned_aac, private};
use crate::{Config, Service};
use axum::http::StatusCode;
use hito_plugin_sdk::{RecordingRequest, SourcePage};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber;

#[tokio::test(flavor = "current_thread")]
async fn socket_audio_delivery_logs_complete_after_the_declared_content_length() {
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
    let app = service.router().layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let dispatch = dispatch.clone();
            async move { next.run(request).with_subscriber(dispatch).await }
        },
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let stopped = CancellationToken::new();
    let shutdown = stopped.clone().cancelled_owned();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .http1_only()
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .post(format!("{base}/v1/sources"))
        .header("connection", "close")
        .json(&RecordingRequest {
            recording_id: "HitoTone001".into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body(response).await;
    private(&bytes, directory.path());
    let page: SourcePage = serde_json::from_slice(&bytes).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    let mut audio = reqwest::Url::parse(&format!("{base}/v1/audio")).unwrap();
    audio
        .query_pairs_mut()
        .append_pair("source_id", &page.items[0].id);
    let response = client
        .get(audio)
        .header("connection", "close")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/mp4");
    assert_eq!(response.content_length(), Some(original.len() as u64));
    assert_eq!(body(response).await.as_slice(), original.as_slice());
    stopped.cancel();
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .await
        .unwrap()
        .unwrap();
    cdn.shutdown().await;

    let output = logs.0.text();
    private(output.as_bytes(), directory.path());
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|event: &Value| event["fields"]["event"] == "youtube_audio_complete")
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["fields"]["outcome"], "complete");
    assert!(events[0]["fields"]["elapsed_ms"].as_u64().is_some());
    assert!(
        events[0]["spans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| { span["operation"] == "audio" && span["request_id"].as_u64().is_some() })
    );
}

async fn body(mut response: reqwest::Response) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.unwrap() {
            assert!(bytes.len() + chunk.len() <= 131_072);
            bytes.extend_from_slice(&chunk);
        }
        bytes
    })
    .await
    .unwrap()
}
