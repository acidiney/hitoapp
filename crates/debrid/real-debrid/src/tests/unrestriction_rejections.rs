use super::{
    logging::Capture,
    provider::{Fixture, ObservedRequest},
};
use crate::{Error, RealDebridClient, TorrentFileBinding};
use serde_json::json;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "private-rejection-provider-token-canary";
const SERVICE_TOKEN: &str = "private-rejection-service-token-canary";
const TORRENT: &str = "private-rejection-torrent-canary";
const HASH: &str = "fedcba9876543210fedcba9876543210fedcba98";
const FILE: &str = "/private-rejection-directory/private-rejection-recording-canary.wav";
const CHANGED_FILE: &str =
    "/private-replacement-directory/private-replacement-recording-canary.wav";
const TITLE: &str = "private-rejection-title-canary";
const HOSTER_LINK: &str = "https://hoster.invalid/private-rejection-link?signature=private-rejection-signature&part=one+two";
const CHANGED_LINK: &str =
    "https://hoster.invalid/private-replacement-link?signature=private-replacement-signature";
const DOWNLOAD_LINK: &str =
    "https://media.invalid/private-rejection-generated-link?token=private-rejection-download-token";
const INVALID_LINK: &str =
    "http://media.invalid/private-rejection-generated-link?token=private-rejection-download-token";
const EXPECTED_FORM: &str = "link=https%3A%2F%2Fhoster.invalid%2Fprivate-rejection-link%3Fsignature%3Dprivate-rejection-signature%26part%3Done%2Btwo";
const FILE_BYTES: u64 = 384_044;

#[tokio::test]
async fn changed_or_unready_bound_torrents_never_dispatch_an_unrestriction_post() {
    let cases = [
        (
            "changed_path",
            torrent("downloaded", 100, CHANGED_FILE, vec![HOSTER_LINK]),
            Error::SourceChanged,
            "source_changed",
        ),
        (
            "downloading",
            torrent("downloading", 25, FILE, vec![]),
            Error::UnsupportedState,
            "unsupported_state",
        ),
    ];
    for (case, body, expected_error, expected_outcome) in cases {
        let fixture = Fixture::start(vec![body]).await;
        let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
            .expect("the owned metadata rejection client must initialize");
        let binding = binding();
        let original_binding = binding.clone();
        let capture = Capture::default();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            client.unrestrict_bound_file(&binding),
        )
        .with_subscriber(capture.dispatch())
        .await;
        let requests = fixture.close().await;
        drop(client);

        let error = result
            .expect("the owned metadata rejection must finish within its deadline")
            .err()
            .expect("changed or unready metadata must not return an unrestriction receipt");
        assert_eq!(
            error, expected_error,
            "{case}: metadata rejection must remain definitive before POST"
        );
        assert!(
            binding == original_binding,
            "{case}: rejection must preserve the exact bound file"
        );
        assert_eq!(
            requests.len(),
            1,
            "{case}: rejected metadata must never dispatch a POST or retry"
        );
        assert!(
            requests[0].method == "GET"
                && requests[0].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
            "{case}: rejection must only fetch the bound torrent metadata"
        );
        assert!(
            requests[0].body.is_empty(),
            "{case}: the metadata GET must have no body"
        );
        assert_private_requests(&requests);
        assert_private_logs(&capture, expected_outcome, case);
    }
}

#[tokio::test]
async fn rejected_or_invalid_unrestriction_acknowledgements_never_return_a_receipt() {
    let valid_body = || acknowledgement(HOSTER_LINK, FILE_BYTES, DOWNLOAD_LINK);
    let error_body = || b"{\"error\":\"private-rejection-provider-error-canary\"}".to_vec();
    let cases = [
        ("unauthorized", 401, error_body(), Error::Status(401)),
        ("forbidden", 403, error_body(), Error::Status(403)),
        (
            "provider_failure",
            500,
            error_body(),
            Error::UnrestrictionUncertain,
        ),
        (
            "malformed_json",
            200,
            b"{\"download\":\"https://media.invalid/private-malformed-download-canary\",".to_vec(),
            Error::UnrestrictionUncertain,
        ),
        (
            "array_response",
            200,
            [b"[".as_slice(), valid_body().as_slice(), b"]".as_slice()].concat(),
            Error::UnrestrictionUncertain,
        ),
        (
            "changed_link_echo",
            200,
            acknowledgement(CHANGED_LINK, FILE_BYTES, DOWNLOAD_LINK),
            Error::UnrestrictionUncertain,
        ),
        (
            "changed_byte_count",
            200,
            acknowledgement(HOSTER_LINK, FILE_BYTES + 1, DOWNLOAD_LINK),
            Error::UnrestrictionUncertain,
        ),
        (
            "non_https_download",
            200,
            acknowledgement(HOSTER_LINK, FILE_BYTES, INVALID_LINK),
            Error::UnrestrictionUncertain,
        ),
    ];
    for (case, status, body, expected_error) in cases {
        let fixture = Fixture::with_acknowledgement(
            torrent("downloaded", 100, FILE, vec![HOSTER_LINK]),
            status,
            body,
        )
        .await;
        let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
            .expect("the owned acknowledgement rejection client must initialize");
        let binding = binding();
        let original_binding = binding.clone();
        let capture = Capture::default();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            client.unrestrict_bound_file(&binding),
        )
        .with_subscriber(capture.dispatch())
        .await;
        let requests = fixture.close().await;
        drop(client);

        let error = result
            .expect("the owned acknowledgement rejection must finish within its deadline")
            .err()
            .expect("an invalid acknowledgement must not return an unrestriction receipt");
        assert_eq!(
            error, expected_error,
            "{case}: acknowledgement classification must preserve uncertainty"
        );
        assert!(
            binding == original_binding,
            "{case}: the acknowledgement must not replace the binding"
        );
        assert_eq!(
            requests.len(),
            2,
            "{case}: an invalid acknowledgement must not cause retries or media requests"
        );
        assert!(
            requests[0].method == "GET"
                && requests[0].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
            "{case}: the first request must revalidate the exact torrent"
        );
        assert!(requests[0].body.is_empty());
        assert!(
            requests[1].method == "POST" && requests[1].target == "/rest/1.0/unrestrict/link",
            "{case}: only one POST may generate the bound hoster link"
        );
        assert!(
            requests[1].body.as_slice() == EXPECTED_FORM.as_bytes(),
            "{case}: the POST must contain exactly the bound link without extra parameters"
        );
        assert_eq!(
            requests[1].header("content-type"),
            Some("application/x-www-form-urlencoded")
        );
        assert!(
            requests[1].header("content-length") == Some(EXPECTED_FORM.len().to_string().as_str()),
            "{case}: the POST must declare the exact captured form length"
        );
        assert_private_requests(&requests);
        let expected_outcome = if matches!(expected_error, Error::Status(_)) {
            "provider_rejected"
        } else {
            "unrestriction_uncertain"
        };
        assert_private_logs(&capture, expected_outcome, case);
    }
}

fn binding() -> TorrentFileBinding {
    TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: FILE_BYTES,
    }
}

fn torrent(status: &str, progress: u16, file_path: &str, links: Vec<&str>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": TORRENT,
        "filename": TITLE,
        "hash": HASH,
        "bytes": FILE_BYTES,
        "progress": progress,
        "status": status,
        "files": [{"id": 7, "path": file_path, "bytes": FILE_BYTES, "selected": 1}],
        "links": links,
        "private_provider_field": DOWNLOAD_LINK
    }))
    .expect("the owned metadata rejection response must serialize")
}

fn acknowledgement(link: &str, bytes: u64, download: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": "private-rejection-download-id-canary",
        "filename": TITLE,
        "mimeType": "audio/wav",
        "filesize": bytes,
        "link": link,
        "download": download,
        "streamable": 1
    }))
    .expect("the owned acknowledgement rejection response must serialize")
}

fn assert_private_requests(requests: &[ObservedRequest]) {
    for request in requests {
        assert!(
            request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()),
            "provider requests must use only the separately supplied provider credential"
        );
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
        assert!(request.header("cookie").is_none());
        assert!(request.header("x-hito-plugin-protocol").is_none());
        assert!(request.header("transfer-encoding").is_none());
    }
}

fn assert_private_logs(capture: &Capture, expected_outcome: &str, case: &str) {
    let logs = capture.text();
    for required in ["request_id", "operation", "outcome", "elapsed_ms"] {
        assert!(
            logs.contains(required),
            "{case}: structured rejection logs must be active"
        );
    }
    let outcomes: Vec<String> = logs
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("the owned rejection log event must be JSON")
        })
        .filter_map(|event| event["fields"]["outcome"].as_str().map(str::to_owned))
        .collect();
    assert!(
        outcomes.len() == 1 && outcomes[0] == expected_outcome,
        "{case}: rejection must report exactly one safe classified outcome"
    );
    for private in [
        TOKEN,
        SERVICE_TOKEN,
        TORRENT,
        HASH,
        FILE,
        CHANGED_FILE,
        TITLE,
        HOSTER_LINK,
        CHANGED_LINK,
        DOWNLOAD_LINK,
        INVALID_LINK,
        EXPECTED_FORM,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "{case}: rejection logs must exclude private provider and binding values"
        );
    }
}
