use super::http::{Capture, Fixture, Reply};
use crate::{ArtistRecording, ArtistRecordings, Error, MusicBrainzClient, RecordingProbe};
use axum::http::StatusCode;
use serde_json::{Value, json};
use tracing::instrument::WithSubscriber;

const ARTIST: &str = "0f700000-0000-4000-8000-0000000000b1";
const PRIVATE_TITLE: &str = "private-recording-title-canary";

fn recording(index: u32, length: Option<u64>) -> Value {
    json!({
        "id": format!("0f700000-0000-4000-8000-{index:012x}"),
        "score": 100,
        "title": format!("{PRIVATE_TITLE} {index}"),
        "length": length,
        "video": false,
        "disambiguation": "",
        "releases": [{"id": "0f700000-0000-4000-8000-0000000000e1", "title": "release"}]
    })
}

fn page(count: u32, offset: u32, recordings: Vec<Value>) -> Reply {
    Reply::json(
        serde_json::to_vec(&json!({
            "created": "2026-10-31T17:00:00.000Z",
            "count": count,
            "offset": offset,
            "recordings": recordings
        }))
        .unwrap(),
    )
}

fn probe(title: &str, length: u64) -> RecordingProbe {
    RecordingProbe {
        title: title.into(),
        min_length_ms: length - 2000,
        max_length_ms: length + 2000,
    }
}

async fn ask(
    reply: Reply,
    probes: &[RecordingProbe],
) -> (Result<ArtistRecordings, Error>, Capture, Vec<String>) {
    let fixture = Fixture::start(vec![reply]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let capture = Capture::default();
    let result = client
        .artist_recordings_matching(ARTIST, probes)
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.requests().iter().map(ToString::to_string).collect();
    fixture.close().await;
    (result, capture, requests)
}

fn decoded(target: &str) -> String {
    reqwest::Url::parse(&format!("http://owned.invalid{target}"))
        .unwrap()
        .query_pairs()
        .find(|(name, _)| name == "query")
        .unwrap()
        .1
        .into_owned()
}

#[tokio::test]
async fn a_probe_search_asks_for_each_title_and_length_range_and_returns_titles_and_lengths() {
    let (result, capture, requests) = ask(
        page(2, 0, vec![recording(1, Some(200_000)), recording(2, None)]),
        &[
            probe("Shape of You", 234_000),
            probe("Say \"Hi\": (Live)", 90_000),
        ],
    )
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
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("/ws/2/recording?query="),
        "{}",
        requests[0]
    );
    assert!(
        requests[0].ends_with("&fmt=json&limit=40&offset=0"),
        "{}",
        requests[0]
    );
    assert_eq!(
        decoded(&requests[0]),
        format!(
            "arid:{ARTIST} AND ((recording:\"Shape of You\" AND dur:[232000 TO 236000]) OR (recording:\"Say \\\"Hi\\\"\\: \\(Live\\)\" AND dur:[88000 TO 92000]))"
        )
    );
    let logs = capture.text();
    assert!(logs.contains("MusicBrainz artist recordings completed"));
    for private in [ARTIST, PRIVATE_TITLE, "Shape of You"] {
        assert!(!logs.contains(private), "logs must not carry {private}");
    }
}

#[tokio::test]
async fn a_search_with_more_matches_than_the_page_is_reported_incomplete() {
    let items: Vec<Value> = (1..=40).map(|index| recording(index, Some(1000))).collect();
    let recordings = ask(page(55, 0, items), &[probe("Song", 100_000)])
        .await
        .0
        .unwrap();
    assert_eq!(recordings.items.len(), 40);
    assert_eq!(recordings.total, 55);
    assert!(!recordings.complete());
}

#[tokio::test]
async fn no_match_is_empty_an_outage_is_typed_and_invalid_answers_are_rejected() {
    let probes = [probe("Song", 100_000)];
    let mut missing = Reply::json(br#"{"error":"Not Found"}"#.to_vec());
    missing.status = StatusCode::NOT_FOUND;
    let empty = ask(missing, &probes).await.0.unwrap();
    assert!(empty.items.is_empty() && empty.complete());
    assert!(ask(page(0, 0, vec![]), &probes).await.0.unwrap().complete());

    let mut outage =
        Reply::json(br#"{"error":"The MusicBrainz web server is currently busy."}"#.to_vec());
    outage.status = StatusCode::SERVICE_UNAVAILABLE;
    let (result, _, requests) = ask(outage, &probes).await;
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
            (1..=41).map(|index| recording(index, Some(1))).collect(),
        ),
    ] {
        assert_eq!(
            ask(invalid, &probes).await.0.unwrap_err(),
            Error::InvalidLookup
        );
    }
}

#[tokio::test]
async fn an_invalid_artist_or_probe_makes_no_request() {
    let fixture = Fixture::start(vec![]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let probes = [probe("Song", 100_000)];
    for invalid in ["nope", "0F700000-0000-4000-8000-0000000000B1", "../artist"] {
        assert_eq!(
            client
                .artist_recordings_matching(invalid, &probes)
                .await
                .unwrap_err(),
            Error::InvalidLookupRequest
        );
    }
    let seven: Vec<RecordingProbe> = (0..7).map(|_| probe("Song", 100_000)).collect();
    for invalid in [
        Vec::new(),
        seven,
        vec![probe("  ", 100_000)],
        vec![RecordingProbe {
            title: "Song".into(),
            min_length_ms: 5,
            max_length_ms: 4,
        }],
        vec![probe(&"x".repeat(513), 100_000)],
    ] {
        assert_eq!(
            client
                .artist_recordings_matching(ARTIST, &invalid)
                .await
                .unwrap_err(),
            Error::InvalidLookupRequest
        );
    }
    assert_eq!(fixture.request_count(), 0);
    fixture.close().await;
}
