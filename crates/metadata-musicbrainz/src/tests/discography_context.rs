use super::{
    ANOTHER_ARTIST_ID, ARTIST_ID,
    http::{Capture, Fixture, Reply},
};
use crate::{Error, MusicBrainzClient};
use hito_plugin_sdk::ArtistDiscographyRequest;
use serde_json::json;
use tracing::instrument::WithSubscriber;

const PRIVATE_SNAPSHOT: &str = "private-discography-snapshot-canary";
const PRIVATE_URL: &str = "private-discography-url-canary?signature=private-signature-canary";

fn request(cursor: Option<String>) -> ArtistDiscographyRequest {
    ArtistDiscographyRequest {
        artist_id: ARTIST_ID.into(),
        cursor,
        snapshot: None,
        limit: 50,
        release_type: None,
    }
}

fn reply(offset: u32, id: &str) -> Reply {
    Reply::json(
        serde_json::to_vec(&json!({
            "release-group-offset": offset,
            "release-group-count": 2,
            "release-groups": [{
                "id": id,
                "title": "Owned context concept",
                "secondary-types": []
            }]
        }))
        .expect("the owned context response must serialize"),
    )
}

#[tokio::test]
async fn returned_cursor_rejects_changed_context_before_http_and_remains_usable() {
    let fixture = Fixture::start(vec![
        reply(0, "0f720000-0000-4000-8000-000000000001"),
        reply(1, "0f720000-0000-4000-8000-000000000002"),
    ])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned continuation client must initialize");
    let capture = Capture::default();
    let dispatch = capture.dispatch();

    let result = async {
        let first = client
            .artist_discography(request(None))
            .with_subscriber(dispatch.clone())
            .await
            .map_err(|_| "the first owned context page must load")?;
        let cursor = first
            .next_cursor
            .ok_or("the first owned context page must offer a continuation")?;
        let before = fixture.request_count();
        let mut rejected = Vec::new();
        for changed in [
            ArtistDiscographyRequest {
                artist_id: ANOTHER_ARTIST_ID.into(),
                ..request(Some(cursor.clone()))
            },
            ArtistDiscographyRequest {
                limit: 25,
                ..request(Some(cursor.clone()))
            },
            ArtistDiscographyRequest {
                snapshot: Some(PRIVATE_SNAPSHOT.into()),
                ..request(Some(cursor.clone()))
            },
            request(Some(format!("{}{PRIVATE_URL}", fixture.endpoint))),
        ] {
            rejected.push(
                client
                    .artist_discography(changed)
                    .with_subscriber(dispatch.clone())
                    .await,
            );
        }
        let after_rejected = fixture.request_count();
        let last = client
            .artist_discography(request(Some(cursor.clone())))
            .with_subscriber(dispatch.clone())
            .await;
        Ok::<_, &'static str>((cursor, before, rejected, after_rejected, last))
    }
    .await;
    drop(client);
    let after = fixture.request_count();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;

    let (cursor, before, rejected, after_rejected, last) =
        result.expect("the owned continuation workflow must initialize");
    assert_eq!(before, 1);
    assert_eq!(after_rejected, 1, "rejected contexts must not reach HTTP");
    for result in rejected {
        let error = result
            .err()
            .expect("a mismatched context or URL cursor must be rejected");
        assert_eq!(error, Error::InvalidDiscographyRequest);
        assert_eq!(
            error.to_string(),
            "Use a valid MusicBrainz artist discography request"
        );
        assert_eq!(format!("{error:?}"), "InvalidDiscographyRequest");
    }
    let last = last.expect("the unchanged original continuation must remain usable");
    assert_eq!(after, 2);
    assert!(last.artist_id == ARTIST_ID);
    assert_eq!(last.items.len(), 1);
    assert!(last.items[0].id == "0f720000-0000-4000-8000-000000000002");
    assert!(last.next_cursor.is_none());
    assert!(last.snapshot.is_none());

    let logs = capture.text();
    assert!(logs.contains("MusicBrainz artist discography completed"));
    assert!(logs.contains("invalid_discography_request"));
    assert!(logs.contains("request_id"));
    for private in [
        PRIVATE_SNAPSHOT,
        PRIVATE_URL,
        "private-signature-canary",
        endpoint.as_str(),
        cursor.as_str(),
    ] {
        assert!(
            !logs.contains(private),
            "continuation logs must exclude private request data"
        );
    }
}
