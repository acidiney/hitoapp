use super::{
    logging::Capture,
    provider::{DeadlineFixture, ObservedRequest},
};
use crate::{Error, RealDebridClient, TorrentFileBinding, TorrentPhase};
use serde_json::json;
use std::time::{Duration, Instant};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

const TOKEN: &str = "private-deadline-provider-token-canary";
const SERVICE_TOKEN: &str = "private-deadline-service-token-canary";
const TORRENT: &str = "private-deadline-torrent-canary";
const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const FILE: &str = "/private-deadline-directory/private-deadline-recording-canary.wav";
const TITLE: &str = "private-deadline-title-canary";
const HOSTER_LINK: &str = "https://hoster.invalid/private-deadline-link?signature=private-deadline-signature&part=one+two";
const GENERATED_LINK: &str =
    "https://media.invalid/private-deadline-generated-link?token=private-deadline-download-token";
const EXPECTED_FORM: &str = "link=https%3A%2F%2Fhoster.invalid%2Fprivate-deadline-link%3Fsignature%3Dprivate-deadline-signature%26part%3Done%2Btwo";
const FILE_BYTES: u64 = 384_044;

#[tokio::test]
async fn unrestriction_uses_one_twelve_second_budget_across_metadata_and_the_received_post() {
    let get_delay = Duration::from_secs(7);
    let mut fixture = DeadlineFixture::start(
        completed_torrent(),
        EXPECTED_FORM.as_bytes().to_vec(),
        get_delay,
    )
    .await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned aggregate deadline client must initialize");
    let retained_client = client.clone();
    let binding = binding();
    let original_binding = binding.clone();
    let capture = Capture::default();
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(14),
        client.unrestrict_bound_file(&binding),
    )
    .with_subscriber(capture.dispatch())
    .await;
    let elapsed = started.elapsed();
    let post_ready =
        tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_post_ready()).await;
    let post_eof = tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_post_eof()).await;
    let following = tokio::time::timeout(Duration::from_secs(3), retained_client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.close().await;
    drop(retained_client);
    drop(client);

    post_ready
        .expect("the provider must receive the complete POST after delayed metadata")
        .expect("the fixture must confirm the exact POST and held response headers");
    post_eof
        .expect(
            "the aggregate timeout must promptly close the held POST while clients remain alive",
        )
        .expect("the fixture must observe POST peer EOF");
    let error = result
        .expect("the client aggregate deadline must finish before the fourteen second guard")
        .err()
        .expect("a held acknowledgement must not return an unrestriction receipt");
    assert_eq!(error, Error::UnrestrictionUncertain);
    assert!(
        elapsed >= Duration::from_millis(11_500) && elapsed < Duration::from_secs(14),
        "the aggregate deadline must finish near twelve seconds, elapsed {elapsed:?}"
    );
    let following = following
        .expect("the same live client must make progress after its aggregate deadline")
        .expect("a following observation must succeed after POST timeout");
    assert!(
        following.binding == original_binding && binding == original_binding,
        "aggregate timeout and recovery must preserve the exact binding"
    );
    assert_eq!(following.phase, TorrentPhase::ProviderCompleted);
    assert_eq!(following.progress_basis_points, Some(10_000));
    assert_requests(&requests);
    assert!(
        requests[1]
            .received_at
            .duration_since(requests[0].received_at)
            >= get_delay,
        "the real metadata exchange must delay the POST by at least seven seconds"
    );
    assert_private_logs(&capture);
    let uncertain_events: Vec<serde_json::Value> = capture
        .text()
        .lines()
        .map(|line| serde_json::from_str(line).expect("the owned deadline event must be JSON"))
        .filter(|event: &serde_json::Value| {
            event["fields"]["outcome"].as_str() == Some("unrestriction_uncertain")
        })
        .collect();
    assert_eq!(
        uncertain_events.len(),
        1,
        "the aggregate timeout must report one uncertainty outcome"
    );
    let request_id = uncertain_events[0]["span"]["request_id"]
        .as_str()
        .expect("the aggregate timeout must have a structured correlation ID");
    let request_id =
        Uuid::parse_str(request_id).expect("the timeout correlation ID must be a UUID");
    assert_eq!(request_id.get_version_num(), 4);
}

#[tokio::test]
async fn dropping_a_received_unrestriction_post_closes_its_peer_and_allows_a_following_observation()
{
    let mut fixture = DeadlineFixture::start(
        completed_torrent(),
        EXPECTED_FORM.as_bytes().to_vec(),
        Duration::ZERO,
    )
    .await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned POST cancellation client must initialize");
    let operation_client = client.clone();
    let binding = binding();
    let original_binding = binding.clone();
    let capture = Capture::default();
    let mut pending = Box::pin(
        operation_client
            .unrestrict_bound_file(&binding)
            .with_subscriber(capture.dispatch()),
    );
    let post_ready = tokio::select! {
        _ = &mut pending => false,
        ready = tokio::time::timeout(Duration::from_secs(3), fixture.wait_for_post_ready()) => {
            matches!(ready, Ok(Ok(())))
        },
    };
    drop(pending);
    let post_eof = tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_post_eof()).await;
    let following = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.close().await;
    drop(operation_client);
    drop(client);

    assert!(
        post_ready,
        "the fixture must receive the complete POST and offer held headers before cancellation"
    );
    post_eof
        .expect(
            "dropping the owned POST future must promptly close its peer while clones remain alive",
        )
        .expect("the fixture must observe the cancelled POST peer EOF");
    let following = following
        .expect("the same live client must make progress after caller cancellation")
        .expect("a following bound observation must succeed after caller cancellation");
    assert!(
        following.binding == original_binding && binding == original_binding,
        "caller cancellation and recovery must preserve the exact binding"
    );
    assert_eq!(following.phase, TorrentPhase::ProviderCompleted);
    assert_eq!(following.progress_basis_points, Some(10_000));
    assert_requests(&requests);
    assert_private_logs(&capture);
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

fn completed_torrent() -> Vec<u8> {
    serde_json::to_vec(&json!({
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
    .expect("the owned lifecycle response must serialize")
}

fn assert_requests(requests: &[ObservedRequest]) {
    assert_eq!(
        requests.len(),
        3,
        "lifecycle recovery must not retry unrestriction or fetch media"
    );
    for index in [0, 2] {
        assert!(
            requests[index].method == "GET"
                && requests[index].target == format!("/rest/1.0/torrents/info/{TORRENT}"),
            "the first and following requests must only observe the bound torrent"
        );
        assert!(requests[index].body.is_empty());
    }
    assert!(requests[1].method == "POST" && requests[1].target == "/rest/1.0/unrestrict/link");
    assert!(
        requests[1].body.as_slice() == EXPECTED_FORM.as_bytes(),
        "the held POST must contain exactly the private bound link"
    );
    assert_eq!(
        requests[1].header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert!(requests[1].header("content-length") == Some(EXPECTED_FORM.len().to_string().as_str()));
    for request in requests {
        assert!(
            request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()),
            "lifecycle requests must use only the separate provider credential"
        );
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
        assert!(request.header("cookie").is_none());
        assert!(request.header("x-hito-plugin-protocol").is_none());
        assert!(request.header("transfer-encoding").is_none());
    }
}

fn assert_private_logs(capture: &Capture) {
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
            "lifecycle and following observation logs must be active"
        );
    }
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
            "lifecycle logs must exclude private binding and provider values"
        );
    }
}
