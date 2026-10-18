use crate::audiobooks_fixture::{Library, Response};
use axum::http::StatusCode;
use hito_core::{Action, AudiobookChapterAnchor, Error, Operation};
use serde_json::{Value, json};
use uuid::Uuid;

const CLIENT: &str = "audiobook-continuation-http-client";

#[tokio::test]
async fn private_continuation_survives_exact_http_replay_and_reaches_owned_inertia_detail() {
    let library = Library::prepare().await;
    let before = library.state().await;
    let path = format!("/api/v1/audiobooks/{}", library.book);
    let grouped = library
        .owner
        .sync(
            &library.app,
            vec![Action::AudiobookCreate {
                playlist_id: library.book,
            }],
        )
        .await;
    let initial = library
        .owner
        .request(&library.app, "GET", &path, Value::Null)
        .await;
    assert_private(&initial, StatusCode::OK);
    let mut expected_book = json!({
        "playlist": before["owner"]["details"][0]["playlist"],
        "chapters": before["owner"]["details"][0]["entries"],
        "bookmarks": []
    });
    assert_eq!(initial.body, expected_book);
    assert!(
        initial.body.get("continuation").is_none(),
        "A legacy book with recording progress must omit continuation until an explicit anchor exists"
    );
    let ordered: Vec<Value> = initial.body["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chapter| chapter["entry_id"].clone())
        .collect();
    assert_eq!(
        ordered,
        json!([library.entries[1], library.entries[2], library.entries[0]])
            .as_array()
            .unwrap()
            .clone()
    );
    assert!(
        before["peer"]["recordings"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|recording| recording["id"] == library.recording.to_string()
                && recording["available"] == true),
        "The peer must have current access to the same recording independently of private book ownership"
    );

    let first = checkpoint(&library, library.entries[2], 35_125, 1.25);
    let first_ack = library
        .owner
        .request(&library.app, "POST", "/api/v1/sync", first.clone())
        .await;
    assert_ack(&first_ack, &first);
    expected_book["continuation"] = json!({
        "chapter": {
            "audiobook_id": library.book,
            "entry_id": library.entries[2],
            "recording_id": library.recording
        },
        "progress": {
            "recording_id": library.recording,
            "position_ms": 35_125,
            "speed": 1.25
        }
    });
    let selected = library
        .owner
        .request(&library.app, "GET", &path, Value::Null)
        .await;
    assert_private(&selected, StatusCode::OK);
    assert_eq!(
        selected.body, expected_book,
        "The HTTP detail must retain the exact duplicate chapter anchor and existing recording progress projection"
    );
    let second = checkpoint(&library, library.entries[0], 42_125, 1.75);
    let second_ack = library
        .owner
        .request(&library.app, "POST", "/api/v1/sync", second.clone())
        .await;
    assert_ack(&second_ack, &second);
    expected_book["continuation"]["chapter"]["entry_id"] = json!(library.entries[0]);
    expected_book["continuation"]["progress"]["position_ms"] = json!(42_125);
    expected_book["continuation"]["progress"]["speed"] = json!(1.75);
    let newer = library
        .owner
        .request(&library.app, "GET", &path, Value::Null)
        .await;
    assert_private(&newer, StatusCode::OK);
    assert_eq!(newer.body, expected_book);
    let before_replay = library.state().await;
    let replay = library
        .owner
        .request(&library.app, "POST", "/api/v1/sync", first.clone())
        .await;
    assert_ack(&replay, &first);
    assert_eq!(replay.body, first_ack.body);
    assert_eq!(
        library.state().await,
        before_replay,
        "Replaying the exact old client, operation IDs and payload must not roll back progress or append events"
    );

    let foreign_read = library
        .peer
        .request(&library.app, "GET", &path, Value::Null)
        .await;
    assert_not_found(&foreign_read);
    let foreign_write = library
        .peer
        .request(
            &library.app,
            "POST",
            "/api/v1/sync",
            checkpoint(&library, library.entries[1], 999, 2.0),
        )
        .await;
    assert_not_found(&foreign_write);
    assert_eq!(
        library.state().await,
        before_replay,
        "A foreign anchor must reject the complete batch, preserving both accounts and the preceding peer recording checkpoint"
    );
    let detail = library
        .owner
        .request(&library.app, "GET", &path, Value::Null)
        .await;
    assert_private(&detail, StatusCode::OK);
    assert_eq!(detail.body, expected_book);
    let page_path = format!("/audiobooks/{}", library.book);
    let page = library.owner.page(&library.app, &page_path).await;
    assert_private(&page, StatusCode::OK);
    assert_eq!(page.headers["x-inertia"], "true");
    assert_eq!(page.headers["vary"], "X-Inertia");
    assert!(
        page.body["component"] == "AudiobookPage"
            && page.body["url"] == page_path
            && page.body["encryptHistory"] == true
            && page.body["clearHistory"] == false
    );
    assert_eq!(
        page.body["props"]["detail"], expected_book,
        "The owned Inertia detail must transport exactly the same continuation as the authenticated API"
    );
    let after = library.state().await;
    let mut accepted: Vec<(&str, Value)> = grouped
        .into_iter()
        .map(|operation| ("owned-audiobook-http-client", operation))
        .collect();
    for batch in [&first, &second] {
        accepted.extend(
            batch["operations"]
                .as_array()
                .unwrap()
                .iter()
                .cloned()
                .map(|operation| (CLIENT, operation)),
        );
    }
    assert_events(&before, &after, &accepted);
    let mut expected = before;
    expected["owner"]["events"] = after["owner"]["events"].clone();
    expected["owner"]["progress"] = json!([{
        "recording_id": library.recording,
        "position_ms": 42_125,
        "speed": 1.75
    }]);
    let history = &mut expected["owner"]["activity"]["history"][0];
    history["position_ms"] = json!(42_125);
    history["speed"] = json!(1.75);
    history["updated_at"] = after["owner"]["activity"]["history"][0]["updated_at"].clone();
    assert_eq!(
        after, expected,
        "Only intended owner progress and events may change; peer state, edits, likes, chapter entries, sources and explicit download absence must remain exact"
    );
    library.assert_original_unchanged();
}

fn checkpoint(library: &Library, entry_id: Uuid, position_ms: i64, speed: f64) -> Value {
    let operations = vec![
        Operation {
            id: Uuid::new_v4(),
            action: Action::ProgressSet {
                recording_id: library.recording,
                position_ms,
                speed,
            },
        },
        Operation {
            id: Uuid::new_v4(),
            action: Action::AudiobookChapterSet {
                chapter: AudiobookChapterAnchor {
                    audiobook_id: library.book,
                    entry_id,
                    recording_id: library.recording,
                },
            },
        },
    ];
    json!({"device_id": CLIENT, "operations": operations})
}

fn assert_private(response: &Response, status: StatusCode) {
    assert_eq!(response.status, status);
    assert_eq!(response.headers["cache-control"], "private, no-store");
}

fn assert_ack(response: &Response, batch: &Value) {
    assert_private(response, StatusCode::OK);
    let ids: Vec<Value> = batch["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|operation| operation["id"].clone())
        .collect();
    assert_eq!(response.body, json!({"acknowledged": ids}));
}

fn assert_not_found(response: &Response) {
    assert_private(response, StatusCode::NOT_FOUND);
    assert_eq!(response.body, json!({"error": Error::NotFound.to_string()}));
}

fn assert_events(before: &Value, after: &Value, accepted: &[(&str, Value)]) {
    let previous = before["owner"]["events"]["operations"].as_array().unwrap();
    let current = after["owner"]["events"]["operations"].as_array().unwrap();
    assert!(
        current.len() == previous.len() + accepted.len()
            && current[..previous.len()] == previous[..]
    );
    for (event, (device, operation)) in current[previous.len()..].iter().zip(accepted) {
        assert!(event["device_id"] == *device && event["operation"] == *operation);
    }
}
