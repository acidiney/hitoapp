use super::{
    ARTIST_ID,
    http::{Capture, Fixture, Reply},
};
use crate::{Error, MusicBrainzClient};
use hito_plugin_sdk::ArtistDiscographyRequest;
use serde_json::json;
use tracing::instrument::WithSubscriber;

const GROUP_ID: &str = "0f730000-0000-4000-8000-000000000001";
const PRIVATE_TITLE: &str = "private-browse-title-canary";
const PRIVATE_URL: &str =
    "https://provider.invalid/private-browse-canary?signature=private-signature-canary";

#[tokio::test]
async fn browse_wire_cap_accepts_exact_mib_and_rejects_declared_or_streamed_overflow() {
    let mut exact = serde_json::to_vec(&json!({
        "release-group-count": 1,
        "release-group-offset": 0,
        "release-groups": [{
            "id": GROUP_ID,
            "title": PRIVATE_TITLE,
            "secondary-types": [],
            "private_provider_field": PRIVATE_URL
        }]
    }))
    .expect("the owned browse wire response must serialize");
    assert!(exact.len() < 1_048_576);
    exact.resize(1_048_576, b' ');
    assert_eq!(exact.len(), 1_048_576);
    let cases = [(false, false), (false, true), (true, false), (true, true)];
    let replies = cases
        .iter()
        .map(|(streamed, oversized)| {
            let mut body = exact.clone();
            if *oversized {
                body.push(b' ');
            }
            assert_eq!(body.len(), 1_048_576 + usize::from(*oversized));
            let mut reply = Reply::json(body);
            reply.streamed = *streamed;
            reply
        })
        .collect();
    let fixture = Fixture::start(replies).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned browse boundary client must initialize");
    let capture = Capture::default();
    let dispatch = capture.dispatch();
    let mut results = Vec::new();
    for _ in cases {
        results.push(
            client
                .artist_discography(ArtistDiscographyRequest {
                    artist_id: ARTIST_ID.into(),
                    cursor: None,
                    snapshot: None,
                    limit: 50,
        release_type: None,
                })
                .with_subscriber(dispatch.clone())
                .await,
        );
    }
    drop(client);
    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;

    assert_eq!(
        requests.len(),
        4,
        "browse calls must neither retry nor crawl"
    );
    for (result, (_, oversized)) in results.into_iter().zip(cases) {
        if oversized {
            let error = result
                .err()
                .expect("a browse body one byte above 1MiB must be rejected");
            assert_eq!(error, Error::ResponseTooLarge);
            assert_eq!(
                error.to_string(),
                "The MusicBrainz response exceeds the size limit"
            );
            assert_eq!(format!("{error:?}"), "ResponseTooLarge");
        } else {
            let page = result.expect("an exact 1MiB browse body must remain valid");
            assert!(page.artist_id == ARTIST_ID);
            assert_eq!(page.total_count, Some(1));
            assert_eq!(page.items.len(), 1);
            assert!(page.items[0].id == GROUP_ID);
            assert!(page.items[0].title == PRIVATE_TITLE);
            assert!(page.next_cursor.is_none());
            assert!(page.snapshot.is_none());
        }
    }
    let logs = capture.text();
    for required in [
        "MusicBrainz artist discography completed",
        "response_too_large",
        "request_id",
        "elapsed_ms",
    ] {
        assert!(
            logs.contains(required),
            "the browse log capture must be active"
        );
    }
    for private in [
        PRIVATE_TITLE,
        PRIVATE_URL,
        "private-signature-canary",
        endpoint.as_str(),
    ] {
        assert!(
            !logs.contains(private),
            "browse logs must exclude private replies and endpoints"
        );
    }
}
