use super::{logging::Capture, provider::Fixture};
use crate::{Error, RealDebridClient, TorrentFileBinding, TorrentPhase};
use std::time::Duration;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn declared_oversized_json_is_rejected_before_the_withheld_body() {
    const TOKEN: &str = "private-declared-body-provider-token-canary";
    const TORRENT: &str = "private-declared-body-torrent-canary";
    const HASH: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const FILE: &str = "/private-declared-body-directory/private-declared-body-file.wav";
    let mut fixture = Fixture::with_held_body(65_537).await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned declared-body client must initialize");
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: 384_044,
    };
    let capture = Capture::default();
    let observation = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    let disconnected = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.wait_for_held_body_disconnect(),
    )
    .await;
    drop(client);
    let requests = fixture.close().await;

    let error = observation
        .expect("the declared size must be refused while its body is withheld")
        .err()
        .expect("an oversized declared body must not become an observation");
    assert_eq!(error, Error::ResponseTooLarge);
    assert_eq!(
        error.to_string(),
        "The Real-Debrid response exceeds the size limit"
    );
    assert_eq!(format!("{error:?}"), "ResponseTooLarge");
    disconnected
        .expect("the rejected held response must release its connection within the deadline")
        .expect("the fixture must observe the rejected connection closing");
    assert_eq!(
        requests.len(),
        1,
        "declared overflow must not trigger a retry or another provider operation"
    );
    assert!(requests[0].method == "GET");
    assert!(
        requests[0].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
        "declared overflow must only query the bound torrent"
    );

    let logs = capture.text();
    for required in [
        "request_id",
        "operation",
        "outcome",
        "elapsed_ms",
        "response_too_large",
    ] {
        assert!(
            logs.contains(required),
            "structured declared-body log capture must be active"
        );
    }
    for private in [
        TOKEN, TORRENT, HASH, FILE, "private-", "http://", "https://",
    ] {
        assert!(
            !logs.contains(private),
            "declared-body logs must exclude credentials and private binding data"
        );
    }
}

#[tokio::test]
async fn exact_limit_chunked_json_preserves_the_bound_observation() {
    const TOKEN: &str = "private-exact-limit-provider-token-canary";
    const TORRENT: &str = "private-exact-limit-torrent-canary";
    const HASH: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const FILE: &str = "/private-exact-limit-directory/private-exact-limit-file.wav";
    const TITLE: &str = "private-exact-limit-title-canary";
    const PRIVATE_URL: &str =
        "https://provider.invalid/private-exact-limit-url?signature=private-exact-limit-signature";
    let mut body = serde_json::to_vec(&serde_json::json!({
        "id": TORRENT,
        "hash": HASH,
        "bytes": 384_044,
        "progress": 25,
        "status": "downloading",
        "files": [{"id": 7, "path": FILE, "bytes": 384_044, "selected": 1}],
        "links": [],
        "filename": TITLE,
        "private_provider_field": PRIVATE_URL
    }))
    .expect("the owned exact-limit response must serialize");
    assert!(
        body.len() < 65_536,
        "the owned JSON must fit before padding"
    );
    body.resize(65_536, b' ');
    let fixture = Fixture::with_chunked_body(body).await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned exact-limit client must initialize");
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: 384_044,
    };
    let capture = Capture::default();
    let observation = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    drop(client);
    let requests = fixture.close().await;

    let observation = observation
        .expect("the complete exact-limit observation must finish within its deadline")
        .expect("valid JSON at the exact body limit must be accepted");
    assert!(
        observation.binding == binding,
        "exact-limit framing must preserve the exact bound file"
    );
    assert_eq!(observation.phase, TorrentPhase::Downloading);
    assert_eq!(observation.progress_basis_points, Some(2500));
    assert_eq!(
        requests.len(),
        1,
        "an exact-limit observation must not trigger retries or another provider operation"
    );
    assert!(requests[0].method == "GET");
    assert!(
        requests[0].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
        "an exact-limit observation must only query the bound torrent"
    );

    let logs = capture.text();
    for required in [
        "request_id",
        "operation",
        "outcome",
        "elapsed_ms",
        "success",
    ] {
        assert!(
            logs.contains(required),
            "structured exact-limit log capture must be active"
        );
    }
    for private in [
        TOKEN,
        TORRENT,
        HASH,
        FILE,
        TITLE,
        PRIVATE_URL,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "exact-limit logs must exclude private request and response data"
        );
    }
}

#[tokio::test]
async fn streamed_json_overflow_is_rejected_before_the_withheld_chunk_terminator() {
    const TOKEN: &str = "private-streamed-body-provider-token-canary";
    const TORRENT: &str = "private-streamed-body-torrent-canary";
    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
    const FILE: &str = "/private-streamed-body-directory/private-streamed-body-file.wav";
    const TITLE: &str = "private-streamed-body-title-canary";
    const PRIVATE_URL: &str =
        "https://provider.invalid/private-streamed-url?signature=private-streamed-signature";
    let mut body = serde_json::to_vec(&serde_json::json!({
        "id": TORRENT,
        "hash": HASH,
        "bytes": 384_044,
        "progress": 25,
        "status": "downloading",
        "files": [{"id": 7, "path": FILE, "bytes": 384_044, "selected": 1}],
        "links": [],
        "filename": TITLE,
        "private_provider_field": PRIVATE_URL
    }))
    .expect("the owned streamed response must serialize");
    assert!(
        body.len() < 65_536,
        "the owned JSON must fit before padding"
    );
    body.resize(65_537, b' ');
    let mut fixture = Fixture::with_held_chunked_body(body).await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned streamed-body client must initialize");
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: 384_044,
    };
    let capture = Capture::default();
    let observation = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    let disconnected = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.wait_for_held_body_disconnect(),
    )
    .await;
    drop(client);
    let requests = fixture.close().await;

    let error = observation
        .expect("streamed overflow must be refused before the chunk terminator arrives")
        .err()
        .expect("streamed overflow must not become an observation");
    assert_eq!(error, Error::ResponseTooLarge);
    assert_eq!(
        error.to_string(),
        "The Real-Debrid response exceeds the size limit"
    );
    assert_eq!(format!("{error:?}"), "ResponseTooLarge");
    disconnected
        .expect("streamed overflow must release its connection within the deadline")
        .expect("the fixture must observe the overflowing connection closing");
    assert_eq!(
        requests.len(),
        1,
        "streamed overflow must not trigger retries or another provider operation"
    );
    assert!(requests[0].method == "GET");
    assert!(
        requests[0].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
        "streamed overflow must only query the bound torrent"
    );

    let logs = capture.text();
    for required in [
        "request_id",
        "operation",
        "outcome",
        "elapsed_ms",
        "response_too_large",
    ] {
        assert!(
            logs.contains(required),
            "structured streamed-body log capture must be active"
        );
    }
    for private in [
        TOKEN,
        TORRENT,
        HASH,
        FILE,
        TITLE,
        PRIVATE_URL,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "streamed-body logs must exclude private request and response data"
        );
    }
}
