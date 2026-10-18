use super::{logging::Capture, provider::Fixture};
use crate::{Error, RealDebridClient, TorrentFileBinding, TorrentPhase};
use serde_json::json;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "private-real-debrid-provider-token-canary";
const TORRENT: &str = "private-torrent-id-canary";
const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const FILE: &str = "/private-directory-canary/private-recording-canary.wav";
const TITLE: &str = "private-torrent-title-canary";
const LINK: &str =
    "https://provider.invalid/private-host-link-canary?signature=private-signature-canary";
const FILE_BYTES: u64 = 384_044;

#[tokio::test]
async fn observes_one_bound_torrent_file_without_resubmitting_or_fetching_audio() {
    let response = |status: &str, progress: u16, links: Vec<&str>| {
        serde_json::to_vec(&json!({
            "id": TORRENT,
            "filename": TITLE,
            "original_filename": TITLE,
            "hash": HASH,
            "bytes": FILE_BYTES,
            "original_bytes": FILE_BYTES,
            "host": "private-hoster-canary",
            "split": 0,
            "progress": progress,
            "status": status,
            "added": "2026-10-18T10:00:00.000Z",
            "files": [{"id": 7, "path": FILE, "bytes": FILE_BYTES, "selected": 1}],
            "links": links
        }))
        .expect("the owned torrent response must serialize")
    };
    let fixture = Fixture::start(vec![
        response("downloading", 25, vec![]),
        response("downloaded", 100, vec![LINK]),
    ])
    .await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned Real-Debrid client must initialize");
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: FILE_BYTES,
    };
    let capture = Capture::default();
    assert_eq!(
        fixture.request_count(),
        0,
        "construction must not contact the provider"
    );

    let downloading = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    let completed = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    drop(client);
    let requests = fixture.close().await;

    let downloading = downloading
        .expect("the first owned observation must finish within its deadline")
        .expect("the bound downloading file must be observed");
    assert!(
        downloading.binding == binding,
        "the downloading observation must retain the exact binding"
    );
    assert_eq!(downloading.phase, TorrentPhase::Downloading);
    assert_eq!(downloading.progress_basis_points, Some(2500));
    let completed = completed
        .expect("the second owned observation must finish within its deadline")
        .expect("the bound completed file must be observed");
    assert!(
        completed.binding == binding,
        "provider completion must retain the exact binding"
    );
    assert_eq!(completed.phase, TorrentPhase::ProviderCompleted);
    assert_eq!(completed.progress_basis_points, Some(10_000));

    assert_eq!(
        requests.len(),
        2,
        "observation must make exactly two read-only provider requests"
    );
    for request in requests {
        assert!(
            request.method == "GET",
            "observation must not submit a transfer or select files"
        );
        assert!(
            request.target == format!("/rest/1.0/torrents/info/{TORRENT}"),
            "observation must only query the bound torrent without unrestriction or media requests"
        );
        assert!(
            request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()),
            "the upstream request must use only the separately supplied provider credential"
        );
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
        assert!(request.header("cookie").is_none());
        assert!(request.header("x-hito-plugin-protocol").is_none());
        assert!(request.header("transfer-encoding").is_none());
        assert!(
            request
                .header("content-length")
                .is_none_or(|length| length == "0")
        );
        assert_eq!(
            request.buffered_body_bytes, 0,
            "observation must not send a request body"
        );
    }

    let logs = capture.text();
    for required in ["request_id", "operation", "outcome", "elapsed_ms"] {
        assert!(
            logs.contains(required),
            "structured observation log capture must be active"
        );
    }
    for private in [
        TOKEN,
        TORRENT,
        HASH,
        FILE,
        TITLE,
        LINK,
        "private-hoster-canary",
        "private-signature-canary",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "observation logs must exclude private provider data"
        );
    }
}

#[tokio::test]
async fn a_completed_torrent_cannot_replace_the_bound_file_path() {
    const CHANGED_FILE: &str = "/private-replacement-canary/private-different-file-canary.wav";
    let response = serde_json::to_vec(&json!({
        "id": TORRENT,
        "filename": TITLE,
        "original_filename": TITLE,
        "hash": HASH,
        "bytes": FILE_BYTES,
        "original_bytes": FILE_BYTES,
        "host": "private-hoster-canary",
        "split": 0,
        "progress": 100,
        "status": "downloaded",
        "added": "2026-10-18T10:00:00.000Z",
        "files": [{"id": 7, "path": CHANGED_FILE, "bytes": FILE_BYTES, "selected": 1}],
        "links": [LINK]
    }))
    .expect("the owned changed-file response must serialize");
    let fixture = Fixture::start(vec![response]).await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned changed-file client must initialize");
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: FILE_BYTES,
    };
    let original_binding = binding.clone();
    let capture = Capture::default();
    let observation = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    drop(client);
    let requests = fixture.close().await;

    let error = observation
        .expect("the changed-file observation must finish within its deadline")
        .err()
        .expect("provider completion must not accept a different file path");
    assert_eq!(error, Error::SourceChanged);
    assert_eq!(
        error.to_string(),
        "The Real-Debrid source no longer matches the bound file"
    );
    assert_eq!(format!("{error:?}"), "SourceChanged");
    assert!(
        binding == original_binding,
        "a rejected provider response must preserve the exact bound file"
    );
    assert_eq!(
        requests.len(),
        1,
        "a changed file must not trigger retries, mutations or media requests"
    );
    assert!(requests[0].method == "GET");
    assert!(
        requests[0].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
        "a changed file must only query the bound torrent"
    );

    let logs = capture.text();
    for required in [
        "request_id",
        "operation",
        "outcome",
        "elapsed_ms",
        "source_changed",
    ] {
        assert!(
            logs.contains(required),
            "structured changed-file log capture must be active"
        );
    }
    for private in [
        TOKEN,
        TORRENT,
        HASH,
        FILE,
        CHANGED_FILE,
        TITLE,
        LINK,
        "private-hoster-canary",
        "private-signature-canary",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "changed-file logs must exclude private binding and provider data"
        );
    }
    assert!(
        !logs.contains("private-"),
        "changed-file logs must exclude shortened private canaries"
    );
}
