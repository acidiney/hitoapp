use super::{logging::Capture, provider::UncertainFixture};
use crate::{Error, RealDebridClient, TorrentFileBinding, TorrentPhase};
use serde_json::json;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn a_received_unrestriction_post_without_acknowledgement_is_uncertain_and_not_retried() {
    const TOKEN: &str = "private-uncertain-provider-token-canary";
    const SERVICE_TOKEN: &str = "private-uncertain-service-token-canary";
    const TORRENT: &str = "private-uncertain-torrent-canary";
    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
    const FILE: &str = "/private-uncertain-directory/private-uncertain-recording-canary.wav";
    const TITLE: &str = "private-uncertain-title-canary";
    const HOSTER_LINK: &str = "https://hoster.invalid/private-uncertain-link?signature=private-uncertain-signature&part=one+two";
    const GENERATED_LINK: &str = "https://media.invalid/private-uncertain-generated-link?token=private-uncertain-download-token";
    const EXPECTED_FORM: &str = "link=https%3A%2F%2Fhoster.invalid%2Fprivate-uncertain-link%3Fsignature%3Dprivate-uncertain-signature%26part%3Done%2Btwo";
    const FILE_BYTES: u64 = 384_044;
    let response = serde_json::to_vec(&json!({
        "id": TORRENT,
        "filename": TITLE,
        "hash": HASH,
        "bytes": FILE_BYTES,
        "progress": 100,
        "status": "downloaded",
        "files": [{"id": 7, "path": FILE, "bytes": FILE_BYTES, "selected": 1}],
        "links": [HOSTER_LINK],
        "private_provider_field": GENERATED_LINK
    }))
    .expect("the owned uncertainty response must serialize");
    let mut fixture = UncertainFixture::start(response, EXPECTED_FORM.as_bytes().to_vec()).await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned uncertainty client must initialize");
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: FILE_BYTES,
    };
    let original_binding = binding.clone();
    let capture = Capture::default();
    let unrestriction = tokio::time::timeout(
        Duration::from_secs(3),
        client.unrestrict_bound_file(&binding),
    )
    .with_subscriber(capture.dispatch())
    .await;
    let post_received =
        tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_post_received()).await;
    let following = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.close().await;
    drop(client);

    post_received
        .expect(
            "the fixture must receive the complete bound POST before closing without a response",
        )
        .expect("the fixture must signal the exact received POST");
    let error = unrestriction
        .expect("the lost acknowledgement must finish within its deadline")
        .err()
        .expect("an unacknowledged POST must not return an unrestriction receipt");
    assert_eq!(error, Error::UnrestrictionUncertain);
    assert_eq!(
        error.to_string(),
        "The Real-Debrid unrestriction acknowledgement is uncertain"
    );
    assert_eq!(format!("{error:?}"), "UnrestrictionUncertain");
    let following = following
        .expect("the same live client must make progress after acknowledgement loss")
        .expect("the following bound observation must succeed");
    assert!(
        following.binding == original_binding && binding == original_binding,
        "acknowledgement loss and recovery must preserve the exact binding"
    );
    assert_eq!(following.phase, TorrentPhase::ProviderCompleted);
    assert_eq!(following.progress_basis_points, Some(10_000));
    assert_eq!(
        requests.len(),
        3,
        "the client must not repeat the POST or fetch media after acknowledgement loss"
    );
    for index in [0, 2] {
        assert!(
            requests[index].method == "GET"
                && requests[index].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
            "the first and following requests must only observe the bound torrent"
        );
        assert!(requests[index].body.is_empty());
    }
    assert!(
        requests[1].method == "POST" && requests[1].target == "/rest/1.0/unrestrict/link",
        "only one request may unrestrict the selected file link"
    );
    assert!(
        requests[1].body.as_slice() == EXPECTED_FORM.as_bytes(),
        "the unacknowledged POST must carry exactly the bound original link"
    );
    assert_eq!(
        requests[1].header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    for request in requests {
        assert!(
            request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()),
            "the recovery workflow must use only the separate provider credential"
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
            "the scoped uncertainty and recovery logs must be active"
        );
    }
    let uncertain_outcomes = logs
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("the owned log event must be JSON")
        })
        .filter(|event| event["fields"]["outcome"].as_str() == Some("unrestriction_uncertain"))
        .count();
    assert_eq!(
        uncertain_outcomes, 1,
        "acknowledgement loss must have one explicit uncertainty outcome"
    );
    for private in [
        TOKEN,
        SERVICE_TOKEN,
        TORRENT,
        HASH,
        FILE,
        TITLE,
        HOSTER_LINK,
        GENERATED_LINK,
        EXPECTED_FORM,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "uncertainty logs must exclude private binding, provider and credential values"
        );
    }
}
