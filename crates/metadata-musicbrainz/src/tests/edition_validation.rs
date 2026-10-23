use super::{
    ARTIST_ID,
    http::{Capture, Fixture, Reply},
};
use crate::{Error, MusicBrainzClient};
use hito_plugin_sdk::EditionTrackListRequest;
use serde_json::{Value, json};
use std::collections::HashSet;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

const EDITION_ID: &str = "0f720000-0000-4000-8000-000000000001";
const OTHER_EDITION_ID: &str = "0f720000-0000-4000-8000-000000000002";
const GROUP_ID: &str = "0f710000-0000-4000-8000-000000000001";
const OTHER_GROUP_ID: &str = "0f710000-0000-4000-8000-000000000002";
const TRACK_ID: &str = "0f730000-0000-4000-8000-000000000001";
const DATA_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000002";
const SECOND_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000003";
const RECORDING_ID: &str = "0f740000-0000-4000-8000-000000000001";
const PRIVATE_TITLE: &str = "private-edition-title-canary";
const PRIVATE_CREDIT: &str = "private-edition-credit-canary";
const PRIVATE_BODY: &str = "private-edition-provider-value?signature=private-edition-signature";
const INVALID_EDITION: &str = "opaque:private-edition-request-canary";
const INVALID_GROUP: &str = "opaque:private-group-request-canary";

fn request() -> EditionTrackListRequest {
    EditionTrackListRequest {
        edition_id: EDITION_ID.into(),
        release_group_id: GROUP_ID.into(),
    }
}

fn edition(edition_id: &str, group_id: &str) -> Value {
    json!({
        "id": edition_id,
        "title": PRIVATE_TITLE,
        "artist-credit": [{
            "name": PRIVATE_CREDIT,
            "joinphrase": "",
            "artist": {"id": ARTIST_ID, "name": PRIVATE_CREDIT}
        }],
        "annotation": PRIVATE_BODY,
        "release-group": {"id": group_id},
        "media": [{
            "position": 1,
            "title": null,
            "format": null,
            "track-count": 1,
            "track-offset": 0,
            "tracks": [{
                "id": TRACK_ID,
                "position": 1,
                "number": "A1",
                "title": PRIVATE_TITLE,
                "length": null,
                "artist-credit": null,
                "recording": {"id": RECORDING_ID}
            }]
        }]
    })
}

#[tokio::test]
async fn data_tracks_do_not_complete_a_partial_normal_track_list() {
    let mut body = edition(EDITION_ID, GROUP_ID);
    body["media"][0]["track-count"] = json!(2);
    body["media"][0]["data-tracks"] = json!([{
        "id": DATA_TRACK_ID,
        "position": 2,
        "number": "2",
        "title": PRIVATE_TITLE,
        "length": null,
        "artist-credit": null,
        "recording": null
    }]);
    let fixture = Fixture::start(vec![Reply::json(
        serde_json::to_vec(&body).expect("the owned partial edition reply must serialize"),
    )])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned partial edition client must initialize");
    let capture = Capture::default();

    let result = client
        .edition_track_list(request())
        .with_subscriber(capture.dispatch())
        .await;
    drop(client);
    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;

    assert_eq!(result.err(), Some(Error::InvalidEdition));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path(), format!("/ws/2/release/{EDITION_ID}"));
    assert_logs(&capture, &["invalid_edition"], (0, 0), endpoint.as_str());
}

#[tokio::test]
async fn complete_leading_data_track_preserves_normal_tracks_at_zero_offset() {
    let mut body = edition(EDITION_ID, GROUP_ID);
    body["media"][0]["track-count"] = json!(2);
    body["media"][0]["tracks"][0]["position"] = json!(2);
    let mut second = body["media"][0]["tracks"][0].clone();
    second["id"] = json!(SECOND_TRACK_ID);
    second["position"] = json!(3);
    second["number"] = json!("A2");
    body["media"][0]["tracks"]
        .as_array_mut()
        .expect("the owned normal track list must be an array")
        .push(second);
    body["media"][0]["data-tracks"] = json!([{
        "id": DATA_TRACK_ID, "position": 1, "number": "Data",
        "title": PRIVATE_TITLE, "length": null,
        "artist-credit": null, "recording": null
    }]);
    let fixture = Fixture::start(vec![Reply::json(
        serde_json::to_vec(&body).expect("the owned leading-data edition must serialize"),
    )])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned leading-data edition client must initialize");
    let capture = Capture::default();
    assert_eq!(
        fixture.request_count(),
        0,
        "client creation must not fetch metadata"
    );

    let result = client
        .edition_track_list(request())
        .with_subscriber(capture.dispatch())
        .await;
    drop(client);
    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;
    let edition = result.expect("a complete leading-data edition must load");

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path(), format!("/ws/2/release/{EDITION_ID}"));
    assert_eq!(
        requests[0].query(),
        Some("fmt=json&inc=recordings+artist-credits+release-groups")
    );
    let observed = endpoint
        .join(&requests[0].to_string())
        .expect("the observed leading-data lookup must be a URL");
    assert_eq!(observed.origin(), endpoint.origin());
    assert_eq!(edition.id, EDITION_ID);
    assert_eq!(edition.release_group_id, GROUP_ID);
    assert_eq!(edition.media.len(), 1);
    let tracks = edition.media[0]
        .tracks
        .iter()
        .map(|track| {
            (
                track.id.as_str(),
                track.position,
                track.number.as_deref(),
                track.kind,
                track.recording_id.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(tracks).expect("ordered occurrences must serialize"),
        json!([
            [DATA_TRACK_ID, 1, "Data", "data", null],
            [TRACK_ID, 2, "A1", "audio", RECORDING_ID],
            [SECOND_TRACK_ID, 3, "A2", "audio", RECORDING_ID]
        ])
    );
    assert_logs(&capture, &["success"], (1, 3), endpoint.as_str());
}

#[tokio::test]
async fn edition_requests_and_responses_keep_the_exact_edition_and_group_context() {
    let replies = [
        edition(OTHER_EDITION_ID, GROUP_ID),
        edition(EDITION_ID, OTHER_GROUP_ID),
    ]
    .into_iter()
    .map(|body| {
        Reply::json(serde_json::to_vec(&body).expect("the owned context reply must serialize"))
    })
    .collect();
    let fixture = Fixture::start(replies).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned edition context client must initialize");
    let capture = Capture::default();
    let dispatch = capture.dispatch();
    let mut rejected_requests = Vec::new();
    for invalid in [
        EditionTrackListRequest {
            edition_id: INVALID_EDITION.into(),
            ..request()
        },
        EditionTrackListRequest {
            release_group_id: INVALID_GROUP.into(),
            ..request()
        },
    ] {
        let result = client
            .edition_track_list(invalid)
            .with_subscriber(dispatch.clone())
            .await;
        rejected_requests.push((result.err(), fixture.request_count()));
    }
    let mut rejected_responses = Vec::new();
    for _ in 0..2 {
        rejected_responses.push(
            client
                .edition_track_list(request())
                .with_subscriber(dispatch.clone())
                .await
                .err(),
        );
    }
    drop(client);
    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;

    for (error, count) in rejected_requests {
        assert_eq!(error, Some(Error::InvalidEditionRequest));
        assert_eq!(
            count, 0,
            "invalid MusicBrainz request IDs must not reach HTTP"
        );
    }
    assert_eq!(rejected_responses, vec![Some(Error::InvalidEdition); 2]);
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request.path(), format!("/ws/2/release/{EDITION_ID}"));
    }
    assert_logs(
        &capture,
        &[
            "invalid_edition_request",
            "invalid_edition_request",
            "invalid_edition",
            "invalid_edition",
        ],
        (0, 0),
        endpoint.as_str(),
    );
}

fn assert_logs(capture: &Capture, outcomes: &[&str], counts: (u64, u64), endpoint: &str) {
    let logs = capture.text();
    let events = logs
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).expect("edition workflow logs must be structured")
        })
        .filter(|event| event["fields"]["message"] == "MusicBrainz edition track list completed")
        .collect::<Vec<_>>();
    assert_eq!(events.len(), outcomes.len());
    let mut requests = HashSet::new();
    for (event, outcome) in events.iter().zip(outcomes) {
        assert_eq!(event["fields"]["outcome"], *outcome);
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
        assert_eq!(event["fields"]["medium_count"], counts.0);
        assert_eq!(event["fields"]["track_count"], counts.1);
        assert_eq!(event["span"]["operation"], "edition_track_list");
        let request = event["span"]["request_id"]
            .as_str()
            .expect("edition attempts need correlation IDs");
        assert!(Uuid::parse_str(request).is_ok());
        assert!(requests.insert(request));
    }
    for private in [
        PRIVATE_TITLE,
        PRIVATE_CREDIT,
        PRIVATE_BODY,
        "private-edition-signature",
        endpoint,
        ARTIST_ID,
        EDITION_ID,
        OTHER_EDITION_ID,
        GROUP_ID,
        OTHER_GROUP_ID,
        TRACK_ID,
        DATA_TRACK_ID,
        SECOND_TRACK_ID,
        RECORDING_ID,
        INVALID_EDITION,
        INVALID_GROUP,
    ] {
        assert!(
            !logs.contains(private),
            "edition logs must omit private requests, metadata and provider bodies"
        );
    }
}
