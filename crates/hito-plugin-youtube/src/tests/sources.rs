mod backpressure;
mod https;
mod identity;
mod large;
#[cfg(target_os = "linux")]
mod shutdown;
mod socket;

use crate::{Config, Service};
use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, Method, Request, StatusCode},
    response::Response,
};
use hito_plugin_sdk::{ProviderRecording, RecordingRequest, SourcePage};
use http_body_util::{BodyExt, Limited};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recording_sources_relay_owned_aac_with_ranges_and_native_decode() {
    let directory = tempfile::tempdir().unwrap();
    let original = owned_aac(directory.path());
    let total = original.len();
    let mut source_id = String::from("youtube:v1:HitoTone001:");
    for byte in format!("140:{total}:1730000000000000").bytes() {
        write!(&mut source_id, "{byte:02x}").unwrap();
    }
    let mut audio_url = reqwest::Url::parse("http://owned.invalid/v1/audio").unwrap();
    audio_url
        .query_pairs_mut()
        .append_pair("source_id", &source_id);
    let audio = format!("{}?{}", audio_url.path(), audio_url.query().unwrap());
    let mut cdn = https::Fixture::start(original.clone()).await;
    let executable = directory.path().join("private-executable-path-sentinel");
    fs::write(&executable, include_str!("../fixtures/recording.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let signed = cdn.url();
    fs::write(
        directory.path().join("recording.json"),
        json!({
            "id":"HitoTone001", "title":"Owned AAC tone", "artist":"Hito test performer",
            "duration":0.25, "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2",
            "vcodec":"none", "protocol":"https", "filesize":total, "url":signed,
            "is_live":false, "live_status":"not_live", "private_raw_json":"private-raw-json-sentinel",
            "http_headers":{"Cookie":"private-extractor-cookie-sentinel","Authorization":"Bearer private-extractor-token-sentinel"},
            "formats":[{
                "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2", "vcodec":"none",
                "protocol":"https", "filesize":total, "url":signed,
                "http_headers":{"Cookie":"private-extractor-cookie-sentinel"}
            }]
        }).to_string(),
    ).unwrap();
    let logs = Capture::default();
    let dispatch = logs.dispatch();
    let service = Service::with_media_client(Config { executable }, cdn.client());
    let app = service.router();
    let input = RecordingRequest {
        recording_id: "HitoTone001".into(),
    };
    let payload = serde_json::to_vec(&input).unwrap();

    let response = exchange(
        &app,
        &dispatch,
        Method::POST,
        "/v1/recordings/detail",
        None,
        &payload,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let detail = bounded_body(response).await;
    private(&detail, directory.path());
    let detail: ProviderRecording = serde_json::from_slice(&detail).unwrap();
    detail.validate().unwrap();
    assert!(
        detail
            == ProviderRecording {
                id: "HitoTone001".into(),
                title: "Owned AAC tone".into(),
                artist: "Hito test performer".into(),
                duration_ms: Some(250),
                release: None,
                disc_number: None,
                track_number: None,
                revision: None,
                attribution: None,
            }
    );
    for _ in 0..2 {
        let response = exchange(&app, &dispatch, Method::POST, "/v1/sources", None, &payload).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = bounded_body(response).await;
        private(&bytes, directory.path());
        let sources: SourcePage = serde_json::from_slice(&bytes).unwrap();
        sources.validate().unwrap();
        assert_eq!(sources.items.len(), 1);
        assert_eq!(sources.items[0].id, source_id);
        assert_eq!(sources.items[0].label, "AAC / M4A");
        assert!(sources.items[0].ready);
    }

    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    representation(&response, StatusCode::OK, total, None);
    let bytes = bounded_body(response).await;
    assert!(
        bytes == original,
        "Full relay changed the owned AAC representation"
    );
    let decoded = directory.path().join("received.m4a");
    fs::write(&decoded, &bytes).unwrap();
    tokio::task::spawn_blocking(move || decode(&decoded))
        .await
        .unwrap();

    let response = exchange(
        &app,
        &dispatch,
        Method::HEAD,
        &audio,
        Some("bytes=16-47"),
        &[],
    )
    .await;
    representation(&response, StatusCode::OK, total, None);
    assert!(bounded_body(response).await.is_empty());
    let response = exchange(
        &app,
        &dispatch,
        Method::GET,
        &audio,
        Some("bytes=16-47"),
        &[],
    )
    .await;
    representation(
        &response,
        StatusCode::PARTIAL_CONTENT,
        32,
        Some(&format!("bytes 16-47/{total}")),
    );
    assert!(bounded_body(response).await.as_ref() == &original[16..48]);
    let response = exchange(
        &app,
        &dispatch,
        Method::GET,
        &audio,
        Some(&format!("bytes={total}-")),
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        response.headers()["content-range"],
        format!("bytes */{total}")
    );
    assert_eq!(response.headers()["content-length"], "0");
    assert!(bounded_body(response).await.is_empty());

    cdn.hold(4);
    let mut held = Vec::new();
    for _ in 0..4 {
        let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
        representation(&response, StatusCode::OK, total, None);
        let mut body = response.into_body();
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
        assert!(prefix == original[..64]);
        held.push(body);
    }
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    private(&bounded_body(response).await, directory.path());
    drop(held.pop().unwrap());
    cdn.closed(1).await;
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    representation(&response, StatusCode::OK, total, None);
    assert!(bounded_body(response).await.as_ref() == original.as_slice());
    drop(held);
    cdn.closed(3).await;

    tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .with_subscriber(dispatch.clone())
        .await
        .unwrap()
        .unwrap();
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    private(&bounded_body(response).await, directory.path());
    let received = cdn.received();
    assert!(received.iter().any(|request| {
        request.method == "GET"
            && request
                .headers
                .get("range")
                .is_some_and(|range| range == "bytes=16-47")
    }));
    for request in received {
        assert!(request.target.starts_with("/videoplayback?"));
        assert_eq!(
            request.headers.get("accept-encoding").map(String::as_str),
            Some("identity")
        );
        for forbidden in [
            "cookie",
            "authorization",
            "proxy-authorization",
            "x-owned-private",
        ] {
            assert!(
                !request.headers.contains_key(forbidden),
                "Relay forwarded private authority"
            );
        }
    }
    cdn.shutdown().await;
    process_policy(directory.path());
    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        !output.contains("private-"),
        "Workflow logs exposed private input"
    );
    assert!(!output.contains(directory.path().to_str().unwrap()));
    assert!(!output.contains(https::HOST));
    assert!(!output.contains("Owned AAC tone"));
    assert!(!output.contains("Hito test performer"));
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for operation in ["detail", "sources", "audio"] {
        assert!(
            events.iter().any(|event| {
                event["fields"]["event"] == "youtube_request_complete"
                    && event["fields"]["elapsed_ms"].as_u64().is_some()
                    && event["spans"].as_array().is_some_and(|spans| {
                        spans.iter().any(|span| {
                            span["operation"] == operation && span["request_id"].as_u64().is_some()
                        })
                    })
            }),
            "Missing safe correlated completion for {operation}"
        );
    }
}

async fn exchange(
    app: &Router,
    dispatch: &tracing::Dispatch,
    method: Method,
    path: &str,
    range: Option<&str>,
    payload: &[u8],
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("authorization", "Bearer private-bearer-sentinel")
        .header("cookie", "private-cookie-sentinel")
        .header("x-owned-private", "private-header-sentinel");
    if let Some(range) = range {
        request = request.header("range", range);
    }
    tokio::time::timeout(
        Duration::from_secs(3),
        app.clone()
            .oneshot(request.body(Body::from(payload.to_vec())).unwrap()),
    )
    .with_subscriber(dispatch.clone())
    .await
    .unwrap()
    .unwrap()
}

async fn bounded_body(response: Response) -> Bytes {
    tokio::time::timeout(
        Duration::from_secs(3),
        Limited::new(response.into_body(), 131_072).collect(),
    )
    .await
    .unwrap()
    .unwrap()
    .to_bytes()
}

fn representation(response: &Response, status: StatusCode, length: usize, range: Option<&str>) {
    assert_eq!(response.status(), status);
    let headers: &HeaderMap = response.headers();
    assert_eq!(headers["content-type"], "audio/mp4");
    assert_eq!(headers["content-length"], length.to_string());
    assert_eq!(headers["accept-ranges"], "bytes");
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(
        headers
            .get("content-range")
            .map(|value| value.to_str().unwrap()),
        range
    );
    for name in ["location", "set-cookie", "content-encoding"] {
        assert!(!headers.contains_key(name));
    }
}

fn private(bytes: &[u8], directory: &Path) {
    let value = std::str::from_utf8(bytes).unwrap();
    assert!(!value.contains("private-"));
    assert!(!value.contains("https://"));
    assert!(!value.contains(directory.to_str().unwrap()));
}

fn owned_aac(directory: &Path) -> Vec<u8> {
    let path = directory.join("owned.m4a");
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
            "0.25",
            "-c:a",
            "aac",
            "-y",
        ])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = Encoder(child);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "Owned AAC fixture could not be encoded");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Owned fixture encoder timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let bytes = bounded_file(&path);
    assert!((64..=131_072).contains(&bytes.len()));
    bytes
}

struct Encoder(std::process::Child);
impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn decode(path: &Path) {
    let mut decoder = hito_audio::Decoder::open(File::open(path).unwrap()).unwrap();
    assert_eq!(decoder.format().sample_rate, 48_000);
    assert_eq!(decoder.format().channels, 1);
    let mut count = 0;
    let mut energy = 0.;
    while let Some(block) = decoder.read().unwrap() {
        assert!(block.samples.iter().all(|sample| sample.is_finite()));
        count += block.samples.len();
        energy += block
            .samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>();
    }
    assert!((11_000..=14_400).contains(&count));
    assert!((0.005..0.01).contains(&(energy / count as f64)));
}

fn bounded_file(path: &Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(131_073)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 131_072);
    bytes
}

fn process_policy(directory: &Path) {
    let calls = bounded_file(&directory.join("calls"));
    assert_eq!(
        calls, b"invocation\n",
        "Detail and Sources repeated an owned extraction"
    );
    let arguments = bounded_file(&directory.join("arguments"));
    let arguments: Vec<_> = arguments
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| std::str::from_utf8(value).unwrap())
        .collect();
    for flag in [
        "--ignore-config",
        "--no-config-locations",
        "--simulate",
        "--skip-download",
        "--no-cookies",
        "--no-cookies-from-browser",
        "--no-plugin-dirs",
        "--no-remote-components",
        "--no-cache-dir",
        "--dump-single-json",
        "--no-playlist",
        "--no-js-runtimes",
    ] {
        assert!(
            arguments.contains(&flag),
            "Missing recording process policy {flag}"
        );
    }
    assert!(!arguments.contains(&"--flat-playlist"));
    assert!(
        arguments
            .windows(2)
            .any(|args| args == ["--js-runtimes", "deno:/usr/local/bin/deno"])
    );
    assert!(arguments.windows(2).any(|args| args
        == [
            "--format",
            "bestaudio[ext=m4a][acodec=mp4a.40.2][vcodec=none][protocol=https]",
        ]));
    assert_eq!(
        &arguments[arguments.len() - 2..],
        ["--", "https://www.youtube.com/watch?v=HitoTone001"]
    );
    let environment = bounded_file(&directory.join("environment"));
    let environment: BTreeMap<_, _> = environment
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| std::str::from_utf8(value).unwrap().split_once('=').unwrap())
        .collect();
    assert_eq!(
        environment,
        BTreeMap::from([
            ("PATH", "/usr/local/bin:/usr/bin:/bin"),
            ("LANG", "C.UTF-8"),
            ("YTDLP_NO_PLUGINS", "1"),
            ("DENO_DIR", "/tmp/deno"),
            ("DENO_NO_UPDATE_CHECK", "1"),
            ("DENO_NO_PACKAGE_JSON", "1"),
            ("TMPDIR", "/tmp"),
        ])
    );
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Capture {
    fn dispatch(&self) -> tracing::Dispatch {
        let captured = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::registry()
                .with(tracing_subscriber::filter::filter_fn(|metadata| {
                    metadata.target() == "hito_plugin_youtube"
                        || metadata.target().starts_with("hito_plugin_youtube::")
                }))
                .with(tracing_subscriber::filter::LevelFilter::TRACE)
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(move || captured.clone()),
                ),
        )
    }
}
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut captured = self.0.lock().unwrap();
        if captured.len() + bytes.len() > 1_048_576 {
            return Err(io::Error::other("Owned log capture exceeded its bound"));
        }
        captured.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
