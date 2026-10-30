use super::http::{Capture, Fixture, Reply};
use crate::{Error, IsrcRecording, MusicBrainzClient};
use axum::http::StatusCode;
use serde_json::{Value, json};
use tracing::instrument::WithSubscriber;

const ISRC: &str = "USRC17607839";
const RECORDING: &str = "0f700000-0000-4000-8000-0000000000a1";
const OTHER_RECORDING: &str = "0f700000-0000-4000-8000-0000000000a2";
const ARTIST: &str = "0f700000-0000-4000-8000-0000000000b1";
const FEATURED: &str = "0f700000-0000-4000-8000-0000000000b2";
const PRIVATE_TITLE: &str = "private-isrc-title-canary";

fn recording(id: &str, artists: &[&str]) -> Value {
    let credit: Vec<Value> = artists
        .iter()
        .map(|artist| json!({"name": "credited", "joinphrase": " feat. ", "artist": {"id": artist, "name": "credited"}}))
        .collect();
    json!({"id": id, "title": PRIVATE_TITLE, "length": 200000, "artist-credit": credit})
}

fn page(recordings: Vec<Value>) -> Reply {
    Reply::json(serde_json::to_vec(&json!({"isrc": ISRC, "recordings": recordings})).unwrap())
}

async fn lookup(reply: Reply) -> (Result<Vec<IsrcRecording>, Error>, Capture, Vec<String>) {
    let fixture = Fixture::start(vec![reply]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let capture = Capture::default();
    let result = client
        .recordings_by_isrc(ISRC)
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.requests().iter().map(ToString::to_string).collect();
    fixture.close().await;
    (result, capture, requests)
}

#[tokio::test]
async fn an_isrc_lookup_returns_each_recording_with_its_distinct_credited_artists() {
    let (result, capture, requests) = lookup(page(vec![
        recording(RECORDING, &[ARTIST, FEATURED, ARTIST]),
        recording(OTHER_RECORDING, &[ARTIST]),
    ]))
    .await;
    assert_eq!(
        result.unwrap(),
        vec![
            IsrcRecording {
                id: RECORDING.into(),
                artist_ids: vec![ARTIST.into(), FEATURED.into()],
            },
            IsrcRecording {
                id: OTHER_RECORDING.into(),
                artist_ids: vec![ARTIST.into()],
            },
        ]
    );
    assert_eq!(
        requests,
        vec![format!("/ws/2/isrc/{ISRC}?fmt=json&inc=artist-credits")]
    );
    let logs = capture.text();
    assert!(logs.contains("MusicBrainz ISRC lookup completed"));
    for private in [ISRC, RECORDING, ARTIST, PRIVATE_TITLE] {
        assert!(!logs.contains(private), "logs must not carry {private}");
    }
}

#[tokio::test]
async fn an_unknown_isrc_is_an_empty_result_and_an_outage_is_a_typed_failure_without_retry() {
    let mut missing = Reply::json(br#"{"error":"Not Found"}"#.to_vec());
    missing.status = StatusCode::NOT_FOUND;
    let (result, _, requests) = lookup(missing).await;
    assert_eq!(result.unwrap(), vec![]);
    assert_eq!(requests.len(), 1);

    let mut outage = Reply::json(b"busy".to_vec());
    outage.status = StatusCode::SERVICE_UNAVAILABLE;
    let (result, _, requests) = lookup(outage).await;
    assert_eq!(result.unwrap_err(), Error::Status(503));
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn responses_that_do_not_describe_the_requested_isrc_are_invalid() {
    let wrong_isrc = Reply::json(
        serde_json::to_vec(&json!({"isrc": "GBAAA2000001", "recordings": []})).unwrap(),
    );
    assert_eq!(
        lookup(wrong_isrc).await.0.unwrap_err(),
        Error::InvalidLookup
    );
    assert_eq!(
        lookup(page(vec![recording("not-an-id", &[ARTIST])]))
            .await
            .0
            .unwrap_err(),
        Error::InvalidLookup
    );
    assert_eq!(
        lookup(page(vec![recording(RECORDING, &["nope"])]))
            .await
            .0
            .unwrap_err(),
        Error::InvalidLookup
    );
    assert_eq!(
        lookup(page(vec![
            recording(RECORDING, &[ARTIST]),
            recording(RECORDING, &[ARTIST])
        ]))
        .await
        .0
        .unwrap_err(),
        Error::InvalidLookup
    );
    let many: Vec<Value> = (0..101)
        .map(|index| recording(&format!("0f700000-0000-4000-8000-{index:012x}"), &[ARTIST]))
        .collect();
    assert_eq!(
        lookup(page(many)).await.0.unwrap_err(),
        Error::InvalidLookup
    );
}

#[tokio::test]
async fn a_recording_without_credited_artists_has_no_artist_ids_and_noncanonical_codes_make_no_request()
 {
    let (result, _, _) = lookup(page(vec![json!({"id": RECORDING, "title": "t"})])).await;
    assert_eq!(
        result.unwrap(),
        vec![IsrcRecording {
            id: RECORDING.into(),
            artist_ids: vec![]
        }]
    );
    let fixture = Fixture::start(vec![]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    for invalid in [
        "usrc17607839",
        "US-RC1-76-07839",
        "USRC1760783",
        "USRC17607839/x",
    ] {
        assert_eq!(
            client.recordings_by_isrc(invalid).await.unwrap_err(),
            Error::InvalidLookupRequest
        );
    }
    assert_eq!(fixture.request_count(), 0);
    fixture.close().await;
}

const PAGE: &str = "https://open.spotify.com/artist/0000000000000000000001";
const OTHER_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000002";

fn related(artists: &[&str]) -> Reply {
    let relations: Vec<Value> = artists
        .iter()
        .map(|id| json!({"type": "streaming", "target-type": "artist", "artist": {"id": id, "name": PRIVATE_TITLE}}))
        .collect();
    Reply::json(
        serde_json::to_vec(&json!({"id": RECORDING, "resource": PAGE, "relations": relations}))
            .unwrap(),
    )
}

async fn artists_for(
    reply: Reply,
    resource: &str,
) -> (Result<Vec<String>, Error>, Capture, Vec<String>) {
    let fixture = Fixture::start(vec![reply]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let capture = Capture::default();
    let result = client
        .artists_by_url(resource)
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.requests().iter().map(ToString::to_string).collect();
    fixture.close().await;
    (result, capture, requests)
}

#[tokio::test]
async fn a_url_lookup_returns_the_distinct_related_artists_and_ignores_other_relations() {
    let mut mixed =
        serde_json::from_slice::<Value>(&related(&[ARTIST, ARTIST, FEATURED]).body).unwrap();
    mixed["relations"].as_array_mut().unwrap().push(json!({
        "type": "other", "target-type": "recording", "recording": {"id": RECORDING}
    }));
    let (result, capture, requests) =
        artists_for(Reply::json(serde_json::to_vec(&mixed).unwrap()), PAGE).await;
    assert_eq!(
        result.unwrap(),
        vec![ARTIST.to_string(), FEATURED.to_string()]
    );
    assert_eq!(
        requests,
        vec![format!(
            "/ws/2/url?resource=https%3A%2F%2Fopen.spotify.com%2Fartist%2F0000000000000000000001&inc=artist-rels&fmt=json"
        )]
    );
    let logs = capture.text();
    assert!(logs.contains("MusicBrainz URL lookup completed"));
    for private in [PAGE, ARTIST, PRIVATE_TITLE, "spotify.com"] {
        assert!(!logs.contains(private), "logs must not carry {private}");
    }
}

#[tokio::test]
async fn an_unlinked_url_is_empty_an_outage_is_typed_and_unsafe_urls_make_no_request() {
    let mut missing = Reply::json(br#"{"error":"Not Found"}"#.to_vec());
    missing.status = StatusCode::NOT_FOUND;
    assert_eq!(
        artists_for(missing, PAGE).await.0.unwrap(),
        Vec::<String>::new()
    );
    let mut outage = Reply::json(b"busy".to_vec());
    outage.status = StatusCode::SERVICE_UNAVAILABLE;
    let (result, _, requests) = artists_for(outage, OTHER_PAGE).await;
    assert_eq!(result.unwrap_err(), Error::Status(503));
    assert_eq!(requests.len(), 1);
    let (result, _, _) = artists_for(related(&["nope"]), PAGE).await;
    assert_eq!(result.unwrap_err(), Error::InvalidLookup);

    let fixture = Fixture::start(vec![]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    for invalid in [
        "http://open.spotify.com/artist/1",
        "https://user:secret@open.spotify.com/artist/1",
        "https://open.spotify.com/artist/1?si=1",
        "https://open.spotify.com/artist/1#x",
        "https://open.spotify.com:8443/artist/1",
        "not a url",
    ] {
        assert_eq!(
            client.artists_by_url(invalid).await.unwrap_err(),
            Error::InvalidLookupRequest
        );
    }
    assert_eq!(fixture.request_count(), 0);
    fixture.close().await;
}
