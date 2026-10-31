use super::http::{Capture, Fixture, Reply};
use crate::{Error, GenreEntity, MusicBrainzClient};
use axum::http::StatusCode;
use serde_json::{Value, json};
use tracing::instrument::WithSubscriber;

const ID: &str = "0f700000-0000-4000-8000-0000000000c1";
const OTHER: &str = "0f700000-0000-4000-8000-0000000000c2";
const PRIVATE_GENRE: &str = "private-genre-canary";

fn genre(name: &str, count: u32) -> Value {
    json!({"id": "0f700000-0000-4000-8000-0000000000d1", "name": name, "count": count, "disambiguation": ""})
}

fn answer(id: &str, genres: Vec<Value>) -> Reply {
    Reply::json(
        serde_json::to_vec(&json!({
            "id": id,
            "name": "Owned entity",
            "genres": genres,
            "tags": [{"name": "free form tag that is not a genre", "count": 9}]
        }))
        .unwrap(),
    )
}

async fn lookup(
    entity: GenreEntity,
    reply: Reply,
) -> (Result<Vec<String>, Error>, Capture, Vec<String>) {
    let fixture = Fixture::start(vec![reply]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let capture = Capture::default();
    let result = client
        .genres(entity, ID)
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.requests().iter().map(ToString::to_string).collect();
    fixture.close().await;
    (result, capture, requests)
}

#[tokio::test]
async fn an_artist_lookup_returns_only_official_genres_strongest_first_and_logs_only_counts() {
    let (result, capture, requests) = lookup(
        GenreEntity::Artist,
        answer(
            ID,
            vec![
                genre("rock", 2),
                genre(PRIVATE_GENRE, 7),
                genre("fado", 2),
                genre("Rock", 1),
            ],
        ),
    )
    .await;
    assert_eq!(
        result.unwrap(),
        vec![PRIVATE_GENRE.to_owned(), "fado".into(), "rock".into()]
    );
    assert_eq!(
        requests,
        vec![format!("/ws/2/artist/{ID}?fmt=json&inc=genres")]
    );
    let logs = capture.text();
    assert!(logs.contains("MusicBrainz genre lookup completed"));
    for private in [ID, PRIVATE_GENRE, "free form tag"] {
        assert!(!logs.contains(private), "logs must not carry {private}");
    }
}

#[tokio::test]
async fn release_groups_and_recordings_use_their_own_paths() {
    let (_, _, requests) = lookup(GenreEntity::ReleaseGroup, answer(ID, vec![])).await;
    assert_eq!(
        requests,
        vec![format!("/ws/2/release-group/{ID}?fmt=json&inc=genres")]
    );
    let (_, _, requests) = lookup(GenreEntity::Recording, answer(ID, vec![])).await;
    assert_eq!(
        requests,
        vec![format!("/ws/2/recording/{ID}?fmt=json&inc=genres")]
    );
}

#[tokio::test]
async fn an_unknown_entity_is_empty_an_outage_is_typed_and_without_retry() {
    let mut missing = Reply::json(br#"{"error":"Not Found"}"#.to_vec());
    missing.status = StatusCode::NOT_FOUND;
    let (result, _, requests) = lookup(GenreEntity::Artist, missing).await;
    assert_eq!(result.unwrap(), Vec::<String>::new());
    assert_eq!(requests.len(), 1);

    let mut outage = Reply::json(b"busy".to_vec());
    outage.status = StatusCode::SERVICE_UNAVAILABLE;
    let (result, _, requests) = lookup(GenreEntity::Artist, outage).await;
    assert_eq!(result.unwrap_err(), Error::Status(503));
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn invalid_answers_are_rejected() {
    let many: Vec<Value> = (0..65)
        .map(|index| genre(&format!("g{index}"), 1))
        .collect();
    for invalid in [
        answer(OTHER, vec![genre("rock", 1)]),
        answer(ID, vec![genre("", 1)]),
        answer(ID, vec![genre(&"x".repeat(200), 1)]),
        answer(ID, many),
        Reply::json(br#"{"id":"0f700000-0000-4000-8000-0000000000c1","genres":"rock"}"#.to_vec()),
        Reply::json(b"not json".to_vec()),
    ] {
        let (result, _, _) = lookup(GenreEntity::Artist, invalid).await;
        assert_eq!(result.unwrap_err(), Error::InvalidLookup);
    }
}

#[tokio::test]
async fn a_noncanonical_identifier_fails_before_any_request() {
    let fixture = Fixture::start(vec![]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    for bad in [
        "",
        "nope",
        &ID.to_uppercase(),
        "00000000-0000-0000-0000-000000000000",
    ] {
        assert_eq!(
            client.genres(GenreEntity::Artist, bad).await.unwrap_err(),
            Error::InvalidLookupRequest
        );
    }
    assert_eq!(fixture.request_count(), 0);
    fixture.close().await;
}
