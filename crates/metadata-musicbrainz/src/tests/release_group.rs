use super::http::{Capture, Fixture, Reply};
use crate::{Error, MusicBrainzClient, ReleaseGroupLink};
use axum::http::StatusCode;
use serde_json::json;
use tracing::instrument::WithSubscriber;

const RELEASE: &str = "0f700000-0000-4000-8000-0000000000a1";
const MERGED: &str = "0f700000-0000-4000-8000-0000000000a2";
const GROUP: &str = "0f700000-0000-4000-8000-0000000000b1";

fn release(id: &str, group: Option<&str>) -> Reply {
    let mut body = json!({"id": id, "title": "private-release-title-canary"});
    if let Some(group) = group {
        body["release-group"] = json!({"id": group, "title": "private-release-title-canary"});
    }
    Reply::json(serde_json::to_vec(&body).unwrap())
}

async fn lookup(
    reply: Reply,
    id: &str,
) -> (
    Result<Option<ReleaseGroupLink>, Error>,
    Capture,
    Vec<String>,
) {
    let fixture = Fixture::start(vec![reply]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let capture = Capture::default();
    let result = client
        .release_group_of(id)
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.requests().iter().map(ToString::to_string).collect();
    fixture.close().await;
    (result, capture, requests)
}

#[tokio::test]
async fn a_release_lookup_names_its_release_group_in_one_request_without_logging_identifiers() {
    let (result, capture, requests) = lookup(release(RELEASE, Some(GROUP)), RELEASE).await;
    assert_eq!(
        result.unwrap(),
        Some(ReleaseGroupLink {
            release_id: RELEASE.into(),
            release_group_id: GROUP.into(),
        })
    );
    assert_eq!(
        requests,
        vec![format!(
            "/ws/2/release/{RELEASE}?fmt=json&inc=release-groups"
        )]
    );
    let logs = capture.text();
    assert!(logs.contains("MusicBrainz release lookup completed"));
    for private in [RELEASE, GROUP, "private-release-title-canary"] {
        assert!(!logs.contains(private), "logs must not carry {private}");
    }
}

#[tokio::test]
async fn unknown_merged_broken_and_unreachable_releases_are_told_apart() {
    let mut missing = Reply::json(br#"{"error":"Not Found"}"#.to_vec());
    missing.status = StatusCode::NOT_FOUND;
    assert_eq!(lookup(missing, RELEASE).await.0.unwrap(), None);
    let mut outage = Reply::json(b"busy".to_vec());
    outage.status = StatusCode::SERVICE_UNAVAILABLE;
    let (result, _, requests) = lookup(outage, RELEASE).await;
    assert_eq!(result.unwrap_err(), Error::Status(503));
    assert_eq!(requests.len(), 1);
    assert_eq!(
        lookup(release(MERGED, Some(GROUP)), RELEASE)
            .await
            .0
            .unwrap()
            .unwrap()
            .release_id,
        MERGED
    );
    for broken in [
        release(RELEASE, None),
        release(RELEASE, Some("not-an-id")),
        release("not-an-id", Some(GROUP)),
    ] {
        assert_eq!(
            lookup(broken, RELEASE).await.0.unwrap_err(),
            Error::InvalidLookup
        );
    }
    let fixture = Fixture::start(vec![]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    assert_eq!(
        client.release_group_of("not-an-id").await.unwrap_err(),
        Error::InvalidLookupRequest
    );
    assert_eq!(fixture.request_count(), 0);
    fixture.close().await;
}

#[tokio::test]
async fn an_oversized_release_answer_is_rejected() {
    let big = vec![b' '; 65_537];
    let (result, _, requests) = lookup(Reply::json(big), RELEASE).await;
    assert_eq!(result.unwrap_err(), Error::ResponseTooLarge);
    assert_eq!(requests.len(), 1);
}
