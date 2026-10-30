use super::http::{Capture, Fixture, Reply};
use crate::{ArtistRecording, ArtistRecordings, Error, MusicBrainzClient};
use axum::http::StatusCode;
use serde_json::{Value, json};
use tracing::instrument::WithSubscriber;

const ARTIST: &str = "0f700000-0000-4000-8000-0000000000b1";
const PRIVATE_TITLE: &str = "private-recording-title-canary";

fn recording(index: u32, length: Option<u64>) -> Value {
    json!({
        "id": format!("0f700000-0000-4000-8000-{index:012x}"),
        "title": format!("{PRIVATE_TITLE} {index}"),
        "length": length,
        "video": false,
        "disambiguation": ""
    })
}

fn page(count: u32, offset: u32, recordings: Vec<Value>) -> Reply {
    Reply::json(
        serde_json::to_vec(&json!({
            "recording-count": count,
            "recording-offset": offset,
            "recordings": recordings
        }))
        .unwrap(),
    )
}

async fn browse(reply: Reply) -> (Result<ArtistRecordings, Error>, Capture, Vec<String>) {
    let fixture = Fixture::start(vec![reply]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let capture = Capture::default();
    let result = client
        .artist_recordings(ARTIST)
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.requests().iter().map(ToString::to_string).collect();
    fixture.close().await;
    (result, capture, requests)
}

#[tokio::test]
async fn an_artist_recordings_browse_returns_titles_and_lengths_of_the_first_page() {
    let (result, capture, requests) = browse(page(
        2,
        0,
        vec![recording(1, Some(200_000)), recording(2, None)],
    ))
    .await;
    let recordings = result.unwrap();
    assert_eq!(recordings.total, 2);
    assert!(recordings.complete());
    assert_eq!(
        recordings.items,
        vec![
            ArtistRecording {
                id: "0f700000-0000-4000-8000-000000000001".into(),
                title: format!("{PRIVATE_TITLE} 1"),
                length_ms: Some(200_000),
            },
            ArtistRecording {
                id: "0f700000-0000-4000-8000-000000000002".into(),
                title: format!("{PRIVATE_TITLE} 2"),
                length_ms: None,
            },
        ]
    );
    assert_eq!(
        requests,
        vec![format!(
            "/ws/2/recording?artist={ARTIST}&fmt=json&limit=100&offset=0"
        )]
    );
    let logs = capture.text();
    assert!(logs.contains("MusicBrainz artist recordings completed"));
    for private in [ARTIST, PRIVATE_TITLE] {
        assert!(!logs.contains(private), "logs must not carry {private}");
    }
}

#[tokio::test]
async fn a_browse_larger_than_one_page_is_reported_incomplete() {
    let items: Vec<Value> = (1..=100)
        .map(|index| recording(index, Some(1000)))
        .collect();
    let recordings = browse(page(250, 0, items)).await.0.unwrap();
    assert_eq!(recordings.items.len(), 100);
    assert_eq!(recordings.total, 250);
    assert!(!recordings.complete());
}

#[tokio::test]
async fn an_unknown_artist_is_empty_an_outage_is_typed_and_invalid_answers_are_rejected() {
    let mut missing = Reply::json(br#"{"error":"Not Found"}"#.to_vec());
    missing.status = StatusCode::NOT_FOUND;
    let empty = browse(missing).await.0.unwrap();
    assert!(empty.items.is_empty() && empty.complete());

    let mut outage = Reply::json(b"busy".to_vec());
    outage.status = StatusCode::SERVICE_UNAVAILABLE;
    let (result, _, requests) = browse(outage).await;
    assert_eq!(result.unwrap_err(), Error::Status(503));
    assert_eq!(requests.len(), 1);

    for invalid in [
        page(1, 5, vec![recording(1, Some(1))]),
        page(0, 0, vec![recording(1, Some(1))]),
        page(2, 0, vec![recording(1, Some(1)), recording(1, Some(1))]),
        page(1, 0, vec![json!({"id": "nope", "title": "t"})]),
        page(
            1,
            0,
            vec![json!({"id": "0f700000-0000-4000-8000-000000000001", "title": ""})],
        ),
        page(
            101,
            0,
            (1..=101).map(|index| recording(index, Some(1))).collect(),
        ),
    ] {
        assert_eq!(browse(invalid).await.0.unwrap_err(), Error::InvalidLookup);
    }
}

#[tokio::test]
async fn a_noncanonical_artist_identifier_makes_no_request() {
    let fixture = Fixture::start(vec![]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    for invalid in ["nope", "0F700000-0000-4000-8000-0000000000B1", "../artist"] {
        assert_eq!(
            client.artist_recordings(invalid).await.unwrap_err(),
            Error::InvalidLookupRequest
        );
    }
    assert_eq!(fixture.request_count(), 0);
    fixture.close().await;
}
