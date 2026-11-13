use super::{
    ANOTHER_ARTIST_ID, ARTIST_ID,
    http::{Capture, Fixture, Reply},
};
use crate::{Error, MusicBrainzClient};
use axum::http::Method;
use futures_util::FutureExt;
use hito_plugin_sdk::ReleaseGroupRequest;
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};
use tracing::instrument::WithSubscriber;

const GROUP: &str = "0f740000-0000-4000-8000-000000000001";
const TITLE: &str = "Owned canonical group concept";
const PRIVATE_URL: &str = "https://provider.invalid/owned-group?signature=private-group-canary";

#[tokio::test]
async fn targeted_release_group_uses_one_exact_get_and_preserves_canonical_metadata() {
    let fixture = Fixture::start(vec![Reply::json(
        serde_json::to_vec(&json!({
            "id": GROUP,
            "title": TITLE,
            "primary-type": "EP",
            "secondary-types": ["Live"],
            "first-release-date": "2020-02",
            "artist-credit": [
                {
                    "name": "Owned credited alias",
                    "joinphrase": " feat. ",
                    "artist": {"id": ARTIST_ID, "name": "Owned canonical performer"}
                },
                {
                    "name": "Owned guest alias",
                    "joinphrase": "",
                    "artist": {"id": ANOTHER_ARTIST_ID, "name": "Owned canonical guest"}
                }
            ],
            "private_provider_field": PRIVATE_URL
        }))
        .expect("the owned canonical group reply must serialize"),
    )])
    .await;
    let capture = Capture::default();
    let outcome = AssertUnwindSafe(async {
        let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
            .expect("the owned targeted group client must initialize");
        let mut rejected = Vec::new();
        for id in [
            GROUP.to_uppercase(),
            GROUP.replace('-', ""),
            PRIVATE_URL.into(),
        ] {
            rejected.push(
                client
                    .release_group(ReleaseGroupRequest {
                        release_group_id: id,
                    })
                    .with_subscriber(capture.dispatch())
                    .await,
            );
        }
        let before = fixture.request_count();
        let group = client
            .release_group(ReleaseGroupRequest {
                release_group_id: GROUP.into(),
            })
            .with_subscriber(capture.dispatch())
            .await;
        drop(client);
        (rejected, before, group)
    })
    .catch_unwind()
    .await;
    let requests = fixture.requests();
    let methods = fixture.methods();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;
    let (rejected, before, group) = match outcome {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(before, 0, "noncanonical group IDs must not dispatch");
    for result in rejected {
        assert!(matches!(result, Err(Error::InvalidReleaseGroupRequest)));
    }
    assert_eq!(methods, [Method::GET]);
    assert_eq!(
        requests.len(),
        1,
        "a targeted lookup must not crawl or retry"
    );
    assert_eq!(
        requests[0].to_string(),
        format!("/ws/2/release-group/{GROUP}?fmt=json&inc=artist-credits")
    );
    assert_eq!(
        serde_json::to_value(group.expect("the exact canonical group must load")).unwrap(),
        json!({
            "id": GROUP,
            "title": TITLE,
            "artist_credit": [
                {
                    "artist": {"id": ARTIST_ID, "name": "Owned canonical performer"},
                    "credited_name": "Owned credited alias",
                    "join_phrase": " feat. "
                },
                {
                    "artist": {"id": ANOTHER_ARTIST_ID, "name": "Owned canonical guest"},
                    "credited_name": "Owned guest alias",
                    "join_phrase": ""
                }
            ],
            "kind": "ep",
            "secondary_types": ["Live"],
            "first_release_date": "2020-02",
            "revision": null
        })
    );
    let logs = capture.text();
    for required in [
        "MusicBrainz release-group lookup completed",
        "invalid_release_group_request",
        "request_id",
        "elapsed_ms",
    ] {
        assert!(
            logs.contains(required),
            "the workflow capture must be active"
        );
    }
    for private in [GROUP, TITLE, PRIVATE_URL, endpoint.as_str()] {
        assert!(
            !logs.contains(private),
            "logs must exclude private protocol data"
        );
    }
}
