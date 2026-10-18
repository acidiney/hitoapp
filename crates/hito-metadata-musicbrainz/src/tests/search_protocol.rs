use super::{
    ARTIST_ID,
    http::{Capture, Fixture, Reply},
};
use crate::{Error, MusicBrainzClient};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

pub(super) const PRIVATE_QUERY: &str = "private-search-query-canary / A+B";
const PRIVATE_NAME: &str = "private-search-name-canary";
const PRIVATE_BODY: &str = "private-search-body-canary";
const PRIVATE_LOCATION: &str =
    "/private-search-redirect-canary?signature=private-search-header-canary";

pub(super) fn request() -> ArtistSearchRequest {
    ArtistSearchRequest {
        query: PRIVATE_QUERY.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    }
}

pub(super) fn wire(offset: u32, count: Value, artists: Value) -> Value {
    json!({"count": count, "offset": offset, "artists": artists})
}

pub(super) fn artist() -> Value {
    json!({"id": ARTIST_ID, "name": PRIVATE_NAME, "disambiguation": null})
}

pub(super) fn reply(wire: Value) -> Reply {
    Reply::json(serde_json::to_vec(&wire).unwrap())
}

fn assert_safe_logs(capture: &Capture, endpoint: &reqwest::Url) {
    let logs = capture.text();
    let completed: Vec<Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|line: &Value| line["fields"]["message"] == "MusicBrainz artist search completed")
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "Each observed search must log one completion"
    );
    let event = &completed[0];
    assert!(event["fields"]["outcome"].is_string());
    assert!(event["fields"]["elapsed_ms"].is_u64());
    assert!(event["fields"]["item_count"].is_u64());
    assert_eq!(event["span"]["operation"], json!("artist_search"));
    let request_id = event["span"]["request_id"]
        .as_str()
        .expect("Search logs need a request ID");
    let parsed = Uuid::parse_str(request_id).expect("The request correlation ID must be a UUID");
    assert!(!parsed.is_nil());
    for private in [
        "private-search-query-canary",
        PRIVATE_NAME,
        PRIVATE_BODY,
        PRIVATE_LOCATION,
        "private-search-header-canary",
        ARTIST_ID,
        endpoint.as_str(),
    ] {
        assert!(
            !logs.contains(private),
            "Search logs must exclude private protocol content"
        );
    }
}

async fn fetch(
    mut response: Reply,
    request: ArtistSearchRequest,
) -> Result<ArtistSearchPage, Error> {
    response.location = response.location.or(Some(PRIVATE_LOCATION));
    let fixture = Fixture::start(vec![response]).await;
    let capture = Capture::default();
    let result = AssertUnwindSafe(async {
        let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
        client
            .artist_search(request)
            .with_subscriber(capture.dispatch())
            .await
    })
    .catch_unwind()
    .await;
    let requests = fixture.request_count();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;
    let result = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        requests, 1,
        "A search must not retry or follow response locations"
    );
    assert_safe_logs(&capture, &endpoint);
    result
}

#[tokio::test]
async fn search_rejects_invalid_response_identity_counts_and_text_without_inventing_metadata() {
    let mut exact_artist = artist();
    exact_artist["name"] = json!("é".repeat(1024));
    exact_artist["disambiguation"] = json!("é".repeat(1024));
    exact_artist["type"] = json!("Unrecognized future artist kind");
    exact_artist["private_provider_field"] = json!(PRIVATE_BODY);
    let bounded_request = ArtistSearchRequest {
        query: "é".repeat(256),
        limit: 20,
        ..request()
    };
    let accepted = fetch(
        reply(wire(0, json!(1), json!([exact_artist.clone()]))),
        bounded_request,
    )
    .await
    .expect("Exact UTF-8 text boundaries must survive the adapter");
    assert_eq!(accepted.query.len(), 512);
    assert_eq!(accepted.items[0].artist.name.len(), 2048);
    assert_eq!(
        accepted.items[0].disambiguation.as_ref().unwrap().len(),
        2048
    );
    assert!(
        accepted.items[0].kind.is_none(),
        "An unknown future kind must remain unknown"
    );

    let mut invalid = vec![
        wire(1, json!(1), json!([artist()])),
        wire(0, json!(0), json!([artist()])),
        wire(0, json!(1), json!([])),
        wire(0, json!(2), json!([artist(), artist()])),
        wire(0, json!(3), json!([artist(), artist(), artist()])),
        json!({"offset": -1, "count": 1, "artists": [artist()]}),
        json!({"offset": 0, "count": -1, "artists": [artist()]}),
        json!({"offset": 0, "count": u64::from(u32::MAX) + 1, "artists": [artist()]}),
        json!({"offset": 0, "count": 1, "artists": null}),
    ];
    for (field, value) in [
        ("id", json!(ARTIST_ID.to_uppercase())),
        ("id", json!(ARTIST_ID.replace('-', ""))),
        ("id", json!("00000000-0000-0000-0000-000000000000")),
        (
            "id",
            json!("https://provider.invalid/private-search-body-canary"),
        ),
        ("name", json!("")),
        ("name", json!(" ")),
        ("name", json!("private-search-name-canary\nvalue")),
        ("name", json!("é".repeat(1025))),
        (
            "disambiguation",
            json!("private-search-body-canary\u{7f}value"),
        ),
        ("disambiguation", json!("é".repeat(1025))),
    ] {
        let mut candidate = artist();
        candidate[field] = value;
        invalid.push(wire(0, json!(1), json!([candidate])));
    }
    for body in invalid {
        assert_eq!(
            fetch(reply(body), request()).await.err(),
            Some(Error::InvalidSearch)
        );
    }
    let oversized_page = wire(
        0,
        json!(20),
        json!(
            (1..=20)
                .map(|index| json!({
                    "id": format!("0f790000-0000-4000-8000-{index:012}"),
                    "name": "\"".repeat(2048), "disambiguation": "\\".repeat(2048)
                }))
                .collect::<Vec<_>>()
        ),
    );
    assert_eq!(
        fetch(
            reply(oversized_page),
            ArtistSearchRequest {
                limit: 20,
                ..request()
            }
        )
        .await
        .err(),
        Some(Error::InvalidSearch)
    );
}

#[tokio::test]
async fn search_wire_cap_accepts_exact_256_kib_and_rejects_declared_or_streamed_overflow() {
    let mut exact = serde_json::to_vec(&wire(0, json!(1), json!([artist()]))).unwrap();
    exact.resize(262_144, b' ');
    for streamed in [false, true] {
        let mut accepted = Reply::json(exact.clone());
        accepted.streamed = streamed;
        let page = fetch(accepted, request())
            .await
            .expect("An exact 256KiB wire page must be accepted");
        assert_eq!(page.items.len(), 1);
        let mut oversized = exact.clone();
        oversized.push(b' ');
        let mut rejected = Reply::json(oversized);
        rejected.streamed = streamed;
        let error = fetch(rejected, request())
            .await
            .err()
            .expect("One extra wire byte must be rejected");
        assert_eq!(error, Error::ResponseTooLarge);
        assert_eq!(
            error.to_string(),
            "The MusicBrainz response exceeds the size limit"
        );
        assert_eq!(format!("{error:?}"), "ResponseTooLarge");
    }
}

#[tokio::test]
async fn search_rejects_wrong_mime_malformed_json_redirects_and_status_bodies_without_following() {
    let mut wrong_mime = reply(wire(0, json!(1), json!([artist()])));
    wrong_mime.mime = "text/html; private=private-search-header-canary";
    let malformed = Reply::json(PRIVATE_BODY.as_bytes().to_vec());
    let mut redirect = Reply::json(PRIVATE_BODY.as_bytes().to_vec());
    redirect.status = StatusCode::FOUND;
    redirect.location = Some(PRIVATE_LOCATION);
    let mut unavailable = Reply::json(PRIVATE_BODY.as_bytes().to_vec());
    unavailable.status = StatusCode::SERVICE_UNAVAILABLE;
    for (response, expected) in [
        (wrong_mime, Error::InvalidSearch),
        (malformed, Error::InvalidSearch),
        (redirect, Error::Status(302)),
        (unavailable, Error::Status(503)),
    ] {
        let error = fetch(response, request())
            .await
            .err()
            .expect("Invalid search protocol data must be rejected");
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("private-search"));
        assert!(!format!("{error:?}").contains("private-search"));
    }
}
