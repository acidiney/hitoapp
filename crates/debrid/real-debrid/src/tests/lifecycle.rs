use super::{
    logging::Capture,
    provider::{ConcurrentFixture, Fixture},
};
use crate::{Error, RealDebridClient, TorrentFileBinding, TorrentPhase};
use std::time::{Duration, Instant};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn dropping_a_held_observation_closes_its_connection_and_allows_the_next_observation() {
    const TOKEN: &str = "private-cancellation-provider-token-canary";
    const TORRENT: &str = "private-cancellation-torrent-canary";
    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
    const FILE: &str = "/private-cancellation-directory/private-cancellation-file.wav";
    const TITLE: &str = "private-cancellation-title-canary";
    const PRIVATE_URL: &str = "https://provider.invalid/private-cancellation-url?signature=private-cancellation-signature";
    let response = serde_json::to_vec(&serde_json::json!({
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
    .expect("the owned post-cancellation response must serialize");
    let mut fixture = Fixture::with_held_body_then_json(1024, response).await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned cancellation client must initialize");
    let cancelling_client = client.clone();
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: 384_044,
    };
    let capture = Capture::default();
    let mut pending = Box::pin(
        cancelling_client
            .observe(&binding)
            .with_subscriber(capture.dispatch()),
    );
    let held_body_started = tokio::select! {
        _ = &mut pending => false,
        started = tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_held_body_start()) => {
            matches!(started, Ok(Ok(())))
        },
    };
    drop(pending);
    let disconnected = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.wait_for_held_body_disconnect(),
    )
    .await;
    let next = tokio::time::timeout(Duration::from_secs(3), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    drop(cancelling_client);
    drop(client);
    let requests = fixture.close().await;

    assert!(
        held_body_started,
        "the provider must receive the GET and offer a held within-limit body before cancellation"
    );
    disconnected
        .expect("dropping the held observation must promptly release its connection")
        .expect("the fixture must observe the cancelled connection closing");
    let next = next
        .expect("the same live client must make progress after cancellation")
        .expect("the next bound observation must succeed after cancellation");
    assert!(
        next.binding == binding,
        "cancellation must preserve the exact file binding"
    );
    assert_eq!(next.phase, TorrentPhase::Downloading);
    assert_eq!(next.progress_basis_points, Some(2500));
    assert_eq!(
        requests.len(),
        2,
        "cancellation must not create retries, mutations or media requests"
    );
    for request in requests {
        assert!(request.method == "GET");
        assert!(
            request.target == format!("/rest/1.0/torrents/info/{TORRENT}"),
            "both observations must only query the bound torrent"
        );
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
            "the successful post-cancellation observation must have active scoped logs"
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
            "cancellation and follow-up logs must exclude private request and response data"
        );
    }
}

#[tokio::test]
async fn clones_share_capacity_and_recover_both_slots_after_cancellation() {
    const TOKEN: &str = "private-capacity-provider-token-canary";
    const TORRENT: &str = "private-capacity-torrent-canary";
    const HASH: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const FILE: &str = "/private-capacity-directory/private-capacity-file.wav";
    let mut fixture = ConcurrentFixture::start().await;
    let client = RealDebridClient::with_test_endpoint(TOKEN.into(), fixture.endpoint.clone())
        .expect("the owned capacity client must initialize");
    let first_client = client.clone();
    let second_client = client.clone();
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: 384_044,
    };
    let capture = Capture::default();
    let mut first = Box::pin(
        first_client
            .observe(&binding)
            .with_subscriber(capture.dispatch()),
    );
    let mut second = Box::pin(
        second_client
            .observe(&binding)
            .with_subscriber(capture.dispatch()),
    );
    let first_round_started = Instant::now();
    let first_pair_started = tokio::select! {
        _ = &mut first => false,
        _ = &mut second => false,
        started = tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_starts(2)) => {
            matches!(started, Ok(Ok(())))
        },
    };
    let busy = tokio::time::timeout(Duration::from_secs(1), client.observe(&binding))
        .with_subscriber(capture.dispatch())
        .await;
    drop(first);
    drop(second);
    let first_pair_disconnected =
        tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_disconnects(2)).await;

    let mut first = Box::pin(
        first_client
            .observe(&binding)
            .with_subscriber(capture.dispatch()),
    );
    let mut second = Box::pin(
        second_client
            .observe(&binding)
            .with_subscriber(capture.dispatch()),
    );
    let second_round_started = Instant::now();
    let second_pair_started = tokio::select! {
        _ = &mut first => false,
        _ = &mut second => false,
        started = tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_starts(4)) => {
            matches!(started, Ok(Ok(())))
        },
    };
    drop(first);
    drop(second);
    let second_pair_disconnected =
        tokio::time::timeout(Duration::from_secs(1), fixture.wait_for_disconnects(4)).await;
    drop(first_client);
    drop(second_client);
    drop(client);
    let mut requests = fixture.close().await;

    assert!(
        first_pair_started,
        "both clone observations must remain admitted concurrently"
    );
    let busy = busy
        .expect("a third observation must be rejected promptly")
        .err()
        .expect("a third observation must not be admitted while both slots are held");
    assert_eq!(busy, Error::Busy);
    assert_eq!(busy.to_string(), "The Real-Debrid client is busy");
    assert_eq!(format!("{busy:?}"), "Busy");
    first_pair_disconnected
        .expect("both initial cancelled connections must close promptly")
        .expect("the fixture must observe both initial connections closing");
    assert!(
        second_pair_started,
        "both admission slots must be reusable concurrently after cancellation"
    );
    second_pair_disconnected
        .expect("both recovered cancelled connections must close promptly")
        .expect("the fixture must observe both recovered connections closing");
    assert_eq!(
        requests.len(),
        4,
        "Busy must not contact the provider or add retry, mutation or media requests"
    );
    requests.sort_by_key(|request| request.received_at);
    let precision = Duration::from_millis(5);
    let pacing_window = Duration::from_millis(250);
    assert!(
        requests[1].received_at.duration_since(first_round_started) + precision >= pacing_window,
        "the second GET must not arrive before the first round's paced pre-poll window"
    );
    assert!(
        requests[3].received_at.duration_since(second_round_started) + precision >= pacing_window,
        "the fourth GET must not arrive before the recovered round's paced pre-poll window"
    );
    for request in requests {
        assert!(request.method == "GET");
        assert!(request.target == format!("/rest/1.0/torrents/info/{TORRENT}"));
    }
    let logs = capture.text();
    for required in ["request_id", "operation", "outcome", "elapsed_ms", "busy"] {
        assert!(
            logs.contains(required),
            "the Busy observation must have active scoped logs"
        );
    }
    for private in [
        TOKEN, TORRENT, HASH, FILE, "private-", "http://", "https://",
    ] {
        assert!(
            !logs.contains(private),
            "capacity logs must exclude private provider data"
        );
    }
}
