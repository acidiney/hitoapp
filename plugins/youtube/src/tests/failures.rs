use crate::{Config, router};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

const NETWORK: &str = "Hito cannot reach YouTube";
const REFUSED: &str = "YouTube refused the request";
const VIDEO_UNAVAILABLE: &str = "The YouTube video is unavailable";
const FORMAT_MISSING: &str = "The YouTube video has no supported audio format";
const OTHER: &str = "The YouTube service is unavailable";

struct Case {
    stderr: &'static str,
    status: StatusCode,
    message: &'static str,
    outcome: &'static str,
}

fn cases() -> Vec<Case> {
    let network = |stderr| Case {
        stderr,
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: NETWORK,
        outcome: "network",
    };
    let refused = |stderr| Case {
        stderr,
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: REFUSED,
        outcome: "refused",
    };
    let missing = |stderr| Case {
        stderr,
        status: StatusCode::NOT_FOUND,
        message: VIDEO_UNAVAILABLE,
        outcome: "video_unavailable",
    };
    vec![
        network(
            "ERROR: [youtube] x: Unable to download webpage: <urlopen error [Errno -3] Temporary failure in name resolution>",
        ),
        network(
            "ERROR: [youtube] x: Unable to download API page: [Errno -2] Name or service not known",
        ),
        network(
            "ERROR: Unable to download webpage: <urlopen error [Errno 101] Network is unreachable>",
        ),
        network("ERROR: [youtube] x: Unable to download webpage: [Errno 111] Connection refused"),
        refused(
            "ERROR: [youtube] x: Sign in to confirm you're not a bot. Use --cookies-from-browser",
        ),
        refused("ERROR: [youtube] x: Unable to download webpage: HTTP Error 403: Forbidden"),
        refused(
            "ERROR: [youtube] x: Unable to download API page: HTTP Error 429: Too Many Requests",
        ),
        missing("ERROR: [youtube] x: Video unavailable. This video is not available"),
        missing(
            "ERROR: [youtube] x: Private video. Sign in if you've been granted access to this video",
        ),
        missing(
            "ERROR: [youtube] x: The uploader has not made this video available in your country",
        ),
        Case {
            stderr: "ERROR: [youtube] x: Requested format is not available. Use --list-formats for a list of available formats",
            status: StatusCode::NOT_FOUND,
            message: FORMAT_MISSING,
            outcome: "format_missing",
        },
        Case {
            stderr: "ERROR: something nobody has classified yet",
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: OTHER,
            outcome: "unavailable",
        },
        Case {
            stderr: "",
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: OTHER,
            outcome: "unavailable",
        },
    ]
}

#[tokio::test]
async fn extractor_failures_are_classified_from_stderr_without_logging_it() {
    for case in cases() {
        for (path, body, event) in [
            (
                "/v1/search",
                json!({"query":"private-query-sentinel","cursor":null,"limit":1}),
                "youtube_search_complete",
            ),
            (
                "/v1/recordings/detail",
                json!({"recording_id":"HitoTone001"}),
                "youtube_recording_complete",
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let executable = directory.path().join("owned-yt-dlp");
            fs::write(&executable, include_str!("../fixtures/failing.sh")).unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(
                directory.path().join("stderr"),
                format!("{} private-stderr-sentinel", case.stderr),
            )
            .unwrap();
            fs::write(directory.path().join("status"), "1").unwrap();
            let logs = LogCapture::unbounded();
            let captured = logs.clone();
            let dispatch = tracing::Dispatch::new(
                tracing_subscriber::registry()
                    .with(tracing_subscriber::filter::LevelFilter::TRACE)
                    .with(
                        tracing_subscriber::fmt::layer()
                            .json()
                            .with_writer(move || captured.clone()),
                    ),
            );
            let app: Router = router(Config { executable });
            let response = app
                .oneshot(
                    Request::post(path)
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .with_subscriber(dispatch)
                .await
                .unwrap();
            assert_eq!(response.status(), case.status, "{}", case.stderr);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                json!({"error": case.message}),
                "{}",
                case.stderr
            );
            let output = logs.text();
            assert!(!output.contains("private-"), "logs exposed private text");
            assert!(!output.contains("Errno"), "logs exposed stderr text");
            let completion = output
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|entry| entry["fields"]["event"] == event)
                .expect("completion event");
            assert_eq!(
                completion["fields"]["outcome"], case.outcome,
                "{}",
                case.stderr
            );
        }
    }
}

#[tokio::test]
async fn oversized_stderr_still_classifies_from_its_bounded_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../fixtures/failing.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut stderr = String::from("ERROR: Temporary failure in name resolution\n");
    stderr.push_str(&"WARNING: noise\n".repeat(10_000));
    fs::write(directory.path().join("stderr"), stderr).unwrap();
    fs::write(directory.path().join("status"), "1").unwrap();
    let response = router(Config { executable })
        .oneshot(
            Request::post("/v1/search")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"query":"tune","cursor":null,"limit":1}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        json!({"error": NETWORK})
    );
}
