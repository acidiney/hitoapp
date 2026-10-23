use super::{
    ANOTHER_ARTIST_ID,
    http::Fixture,
    search_protocol::{PRIVATE_QUERY, artist, reply, request, wire},
};
use crate::{Error, MusicBrainzClient};
use futures_util::FutureExt;
use hito_plugin_sdk::ArtistSearchRequest;
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};

#[tokio::test]
async fn invalid_search_requests_and_changed_cursor_context_never_reach_http() {
    let mut last_artist = artist();
    last_artist["id"] = json!(ANOTHER_ARTIST_ID);
    let fixture = Fixture::start(vec![
        reply(wire(0, json!(2), json!([artist()]))),
        reply(wire(1, json!(2), json!([last_artist]))),
    ])
    .await;
    let outcome = AssertUnwindSafe(async {
        let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
        let mut rejected = Vec::new();
        for query in [
            "".into(),
            " ".into(),
            "\u{a0}".into(),
            "private-search-query-canary\nvalue".into(),
            "é".repeat(257),
        ] {
            rejected.push(
                client
                    .artist_search(ArtistSearchRequest { query, ..request() })
                    .await,
            );
        }
        for limit in [0, 21] {
            rejected.push(
                client
                    .artist_search(ArtistSearchRequest { limit, ..request() })
                    .await,
            );
        }
        rejected.push(
            client
                .artist_search(ArtistSearchRequest {
                    snapshot: Some("private-search-snapshot-canary".into()),
                    ..request()
                })
                .await,
        );
        for cursor in [
            "",
            " ",
            "private-search-cursor-canary",
            "https://provider.invalid/private-search-cursor-canary",
        ] {
            rejected.push(
                client
                    .artist_search(ArtistSearchRequest {
                        cursor: Some(cursor.into()),
                        ..request()
                    })
                    .await,
            );
        }
        let before_valid = fixture.request_count();
        let first = client
            .artist_search(request())
            .await
            .expect("The owned first page must load");
        let cursor = first
            .next_cursor
            .expect("A short page must advance by its actual item count");
        let retained = || ArtistSearchRequest {
            cursor: Some(cursor.clone()),
            ..request()
        };
        for changed in [
            ArtistSearchRequest {
                query: "private-search-query-canary / a+b".into(),
                ..retained()
            },
            ArtistSearchRequest {
                query: format!("{PRIVATE_QUERY} "),
                ..retained()
            },
            ArtistSearchRequest {
                limit: 1,
                ..retained()
            },
            ArtistSearchRequest {
                snapshot: Some("private-search-snapshot-canary".into()),
                ..retained()
            },
        ] {
            rejected.push(client.artist_search(changed).await);
        }
        let (prefix, _) = cursor.rsplit_once(':').unwrap();
        for invalid_offset in ["0", "01", "+1", "-1", "4294967296", "1:extra", "invalid"] {
            rejected.push(
                client
                    .artist_search(ArtistSearchRequest {
                        cursor: Some(format!("{prefix}:{invalid_offset}")),
                        ..request()
                    })
                    .await,
            );
        }
        rejected.push(
            client
                .artist_search(ArtistSearchRequest {
                    cursor: Some(cursor.replacen("mb-artist-search-v1", "mb-artist-search-v2", 1)),
                    ..request()
                })
                .await,
        );
        let after_rejected = fixture.request_count();
        let last = client.artist_search(retained()).await;
        (before_valid, rejected, after_rejected, last)
    })
    .catch_unwind()
    .await;
    let requests = fixture.requests();
    fixture.close().await;
    let (before_valid, rejected, after_rejected, last) = match outcome {
        Ok(value) => value,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(before_valid, 0);
    assert_eq!(
        after_rejected, 1,
        "Invalid requests and changed continuations must not contact the provider"
    );
    for result in rejected {
        let error = result
            .err()
            .expect("Invalid request context must be rejected");
        assert_eq!(error, Error::InvalidSearchRequest);
        assert_eq!(
            error.to_string(),
            "Use a valid MusicBrainz artist search request"
        );
        assert_eq!(format!("{error:?}"), "InvalidSearchRequest");
    }
    let last = last.expect("Rejected contexts must leave the original continuation usable");
    assert_eq!(requests.len(), 2);
    assert_eq!(last.items[0].artist.id, ANOTHER_ARTIST_ID);
    assert!(last.next_cursor.is_none());
    assert!(requests[1].query().unwrap().ends_with("offset=1"));
}

#[tokio::test]
async fn continuation_responses_reject_totals_behind_the_offset_and_checked_offset_overflow() {
    let fixture = Fixture::start(vec![
        reply(wire(0, json!(2), json!([artist()]))),
        reply(wire(1, json!(0), json!([]))),
        reply(wire(u32::MAX, serde_json::Value::Null, json!([artist()]))),
    ])
    .await;
    let outcome = AssertUnwindSafe(async {
        let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
        let first = client.artist_search(request()).await.unwrap();
        let cursor = first.next_cursor.unwrap();
        let behind = client
            .artist_search(ArtistSearchRequest {
                cursor: Some(cursor.clone()),
                ..request()
            })
            .await;
        let (prefix, _) = cursor.rsplit_once(':').unwrap();
        let overflow = client
            .artist_search(ArtistSearchRequest {
                cursor: Some(format!("{prefix}:{}", u32::MAX)),
                ..request()
            })
            .await;
        (behind, overflow)
    })
    .catch_unwind()
    .await;
    let requests = fixture.request_count();
    fixture.close().await;
    let (behind, overflow) = match outcome {
        Ok(value) => value,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(requests, 3);
    assert_eq!(behind.err(), Some(Error::InvalidSearch));
    assert_eq!(overflow.err(), Some(Error::InvalidSearch));
}
