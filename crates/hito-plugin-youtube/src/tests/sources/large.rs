use super::{Capture, Encoder, bounded_body, exchange, https, private, representation};
use crate::{Config, Service};
use axum::http::{Method, StatusCode};
use hito_plugin_sdk::{RecordingRequest, SourcePage};
use http_body_util::BodyExt;
use serde_json::json;
use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX_BYTES: usize = 2 * 1024 * 1024;

#[tokio::test(flavor = "current_thread")]
async fn large_owned_aac_streams_exactly_over_ready_https_records() {
    let directory = tempfile::tempdir().unwrap();
    let original = large_aac(directory.path());
    let mut cdn = https::Fixture::start(original.clone()).await;
    let executable = directory.path().join("owned-recording-extractor");
    fs::write(&executable, include_str!("../../fixtures/recording.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        directory.path().join("recording.json"),
        json!({
            "id":"HitoTone001", "title":"Owned long AAC tone", "artist":"Hito test performer",
            "duration":90, "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2",
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
    let bytes = bounded_body(response).await;
    private(&bytes, directory.path());
    let page: SourcePage = serde_json::from_slice(&bytes).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].ready);
    let mut url = reqwest::Url::parse("http://owned.invalid/v1/audio").unwrap();
    url.query_pairs_mut()
        .append_pair("source_id", &page.items[0].id);
    let audio = format!("{}?{}", url.path(), url.query().unwrap());
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    representation(&response, StatusCode::OK, original.len(), None);
    cdn.written().await;
    let mut body = response.into_body();
    let received = tokio::time::timeout(Duration::from_secs(5), async {
        let mut received = Vec::with_capacity(original.len());
        while let Some(frame) = body.frame().await {
            let bytes = frame
                .expect("A valid HTTPS AAC representation was rejected")
                .into_data()
                .unwrap();
            assert!(!bytes.is_empty());
            assert!(
                bytes.len() <= 65_536,
                "The relay emitted an unbounded frame"
            );
            assert!(received.len() + bytes.len() <= MAX_BYTES);
            received.extend_from_slice(&bytes);
        }
        received
    })
    .await
    .expect("The large owned AAC relay did not finish");
    assert_eq!(received.len(), original.len());
    assert!(
        received == original,
        "The relay changed the owned AAC bytes"
    );
    tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .await
        .unwrap()
        .unwrap();
    cdn.shutdown().await;
    let output = logs.0.text();
    assert!(
        output.lines().any(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            event["fields"]["event"] == "youtube_audio_frame_split"
                && event["fields"]["input_bytes"]
                    .as_u64()
                    .is_some_and(|length| length > 65_536)
                && event["fields"]["output_bytes"] == 65_536
        }),
        "The real HTTPS transfer did not exercise an oversized upstream frame"
    );
}

fn large_aac(directory: &Path) -> Vec<u8> {
    let path = directory.join("owned-long.m4a");
    let child = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "90",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-y",
        ])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = Encoder(child);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "The large owned AAC encoder failed");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "The large owned AAC encoder timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut decoder = hito_audio::Decoder::open(File::open(&path).unwrap()).unwrap();
    assert_eq!(decoder.format().sample_rate, 48_000);
    assert_eq!(decoder.format().channels, 1);
    assert!(!decoder.read().unwrap().unwrap().samples.is_empty());
    let mut bytes = Vec::new();
    File::open(&path)
        .unwrap()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!((1024 * 1024..=MAX_BYTES).contains(&bytes.len()));
    bytes
}
