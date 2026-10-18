use super::{logging::Capture, provider::Fixture};
use crate::{RealDebridClient, TorrentFileBinding};
use serde_json::json;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn unrestricts_only_the_revalidated_completed_bound_file_without_fetching_media() {
    const TOKEN: &str = "private-unrestriction-provider-token-canary";
    const SERVICE_TOKEN: &str = "private-unrestriction-service-token-canary";
    const TORRENT: &str = "private-unrestriction-torrent-canary";
    const HASH: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const FILE: &str =
        "/private-unrestriction-directory/private-unrestriction-recording-canary.wav";
    const TITLE: &str = "private-unrestriction-title-canary";
    const HOST: &str = "private-unrestriction-hoster-canary";
    const HOSTER_LINK: &str = "https://hoster.invalid/private-host-link-canary?signature=private-signature-canary&part=one+two";
    const DOWNLOAD_LINK: &str =
        "https://media.invalid/private-generated-link-canary?token=private-download-token-canary";
    const FILE_BYTES: u64 = 384_044;
    const EXPECTED_FORM: &str = "link=https%3A%2F%2Fhoster.invalid%2Fprivate-host-link-canary%3Fsignature%3Dprivate-signature-canary%26part%3Done%2Btwo";
    let torrent = serde_json::to_vec(&json!({
        "id": TORRENT,
        "filename": TITLE,
        "original_filename": TITLE,
        "hash": HASH,
        "bytes": FILE_BYTES,
        "original_bytes": FILE_BYTES,
        "host": HOST,
        "split": 0,
        "progress": 100,
        "status": "downloaded",
        "files": [{"id": 7, "path": FILE, "bytes": FILE_BYTES, "selected": 1}],
        "links": [HOSTER_LINK]
    }))
    .expect("the owned completed torrent must serialize");
    let unrestricted = serde_json::to_vec(&json!({
        "id": "private-unrestricted-id-canary",
        "filename": "private-unrestriction-recording-canary.wav",
        "mimeType": "audio/wav",
        "filesize": FILE_BYTES,
        "link": HOSTER_LINK,
        "host": HOST,
        "chunks": 1,
        "crc": 1,
        "download": DOWNLOAD_LINK,
        "streamable": 1
    }))
    .expect("the owned unrestriction response must serialize");
    let fixture = Fixture::start(vec![torrent, unrestricted]).await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned unrestriction client must initialize");
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: FILE_BYTES,
    };
    let original_binding = binding.clone();
    let construction_requests = fixture.request_count();
    let capture = Capture::default();
    let receipt = tokio::time::timeout(
        Duration::from_secs(3),
        client.unrestrict_bound_file(&binding),
    )
    .with_subscriber(capture.dispatch())
    .await;
    drop(client);
    let requests = fixture.close().await;

    assert_eq!(
        construction_requests, 0,
        "constructing the client must not contact the provider"
    );
    let receipt = receipt
        .expect("the owned unrestriction must finish within its deadline")
        .expect("the exact completed file must receive an unrestriction receipt");
    assert!(
        receipt.binding == original_binding && binding == original_binding,
        "the diagnostic receipt must preserve the exact bound file"
    );
    assert_eq!(receipt.confirmed_bytes, FILE_BYTES);
    assert_eq!(
        requests.len(),
        2,
        "unrestriction must only revalidate the torrent and generate its bound link without media or transfer mutations"
    );
    assert!(
        requests[0].method == "GET"
            && requests[0].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
        "the first request must revalidate the bound torrent"
    );
    assert!(
        requests[0].body.is_empty(),
        "the revalidation GET must not send a body"
    );
    assert!(
        requests[1].method == "POST" && requests[1].target == "/rest/1.0/unrestrict/link",
        "the second request must only unrestrict the selected file link"
    );
    assert_eq!(
        requests[1].header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert!(
        requests[1].body.as_slice() == EXPECTED_FORM.as_bytes(),
        "the form must contain exactly the private selected link, without password, remote or service credentials"
    );
    assert!(
        requests[1].header("content-length") == Some(EXPECTED_FORM.len().to_string().as_str()),
        "the form request must declare the exact captured body length"
    );
    for request in requests {
        assert!(
            request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()),
            "both API requests must use only the separately supplied provider credential"
        );
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
        assert!(request.header("cookie").is_none());
        assert!(request.header("x-hito-plugin-protocol").is_none());
        assert!(request.header("transfer-encoding").is_none());
    }

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
            "the unrestriction workflow must have active structured scoped logs"
        );
    }
    for private in [
        TOKEN,
        SERVICE_TOKEN,
        TORRENT,
        HASH,
        FILE,
        TITLE,
        HOST,
        HOSTER_LINK,
        DOWNLOAD_LINK,
        EXPECTED_FORM,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "unrestriction logs must exclude private request, response and binding values"
        );
    }
}
