use crate::audiobooks_fixture::Library;
use axum::http::StatusCode;
use hito_core::Action;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn an_authenticated_account_groups_reads_and_bookmarks_existing_chapter_occurrences() {
    let library = Library::prepare().await;
    let before = library.state().await;
    let chapters = &before["owner"]["details"][0];
    let ordered: Vec<Value> = chapters["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chapter| chapter["entry_id"].clone())
        .collect();
    assert!(
        ordered
            == json!([library.entries[1], library.entries[2], library.entries[0]])
                .as_array()
                .unwrap()
                .clone(),
        "The owned playlist must contain exact duplicate recording occurrences in stored chapter order"
    );
    let initial = library
        .owner
        .request(&library.app, "GET", "/api/v1/audiobooks", Value::Null)
        .await;
    assert_eq!(initial.status, StatusCode::OK);
    assert_eq!(initial.headers["cache-control"], "private, no-store");
    assert!(
        initial.body == json!([]),
        "An ungrouped owned playlist must not expose another account's existing private book"
    );

    let marker = Uuid::new_v4();
    let mut accepted = library
        .owner
        .sync(
            &library.app,
            vec![
                Action::AudiobookCreate {
                    playlist_id: library.book,
                },
                Action::AudiobookBookmarkCreate {
                    audiobook_id: library.book,
                    bookmark_id: marker,
                    entry_id: library.entries[2],
                    recording_id: library.recording,
                    position_ms: 250,
                    label: "Private HTTP return point".into(),
                },
            ],
        )
        .await;
    let listed = library
        .owner
        .request(&library.app, "GET", "/api/v1/audiobooks", Value::Null)
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.headers["cache-control"], "private, no-store");
    assert!(
        listed.body == json!([chapters["playlist"]]),
        "The book list must contain only the owner's grouped live playlist, excluding its ungrouped selection and the peer's book"
    );
    let path = format!("/api/v1/audiobooks/{}", library.book);
    let detail = library
        .owner
        .request(&library.app, "GET", &path, Value::Null)
        .await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.headers["cache-control"], "private, no-store");
    let mut expected = json!({
        "playlist": chapters["playlist"], "chapters": chapters["entries"],
        "bookmarks": [{"id": marker, "audiobook_id": library.book, "entry_id": library.entries[2],
            "recording_id": library.recording, "position_ms": 250, "label": "Private HTTP return point"}],
    });
    assert!(
        detail.body == expected,
        "The generated HTTP detail must retain every exact authorized chapter occurrence and the private bookmark's complete anchor"
    );
    let after_create = library.state().await;
    assert_only_events_changed(&before, &after_create, &accepted);

    accepted.extend(
        library
            .owner
            .sync(
                &library.app,
                vec![Action::AudiobookBookmarkRemove {
                    audiobook_id: library.book,
                    bookmark_id: marker,
                }],
            )
            .await,
    );
    expected["bookmarks"] = json!([]);
    let removed = library
        .owner
        .request(&library.app, "GET", &path, Value::Null)
        .await;
    assert_eq!(removed.status, StatusCode::OK);
    assert_eq!(removed.headers["cache-control"], "private, no-store");
    assert!(
        removed.body == expected,
        "Removing a bookmark through sync must retain the book and every original chapter occurrence"
    );
    assert!(
        library.owner.get(&library.app, "/api/v1/audiobooks").await == listed.body,
        "Bookmark removal must not change the private book list"
    );
    assert_only_events_changed(&before, &library.state().await, &accepted);
    library.assert_original_unchanged();
}

fn assert_only_events_changed(before: &Value, after: &Value, accepted: &[Value]) {
    let prior = before["owner"]["events"]["operations"].as_array().unwrap();
    let current = after["owner"]["events"]["operations"].as_array().unwrap();
    assert!(
        current.len() == prior.len() + accepted.len() && current[..prior.len()] == prior[..],
        "The three explicit audiobook actions must append only their accepted events and retain the exact prior history"
    );
    for (event, operation) in current[prior.len()..].iter().zip(accepted) {
        assert!(
            event["device_id"] == "owned-audiobook-http-client" && event["operation"] == *operation,
            "The HTTP adapter must preserve every exact audiobook operation payload"
        );
    }
    let mut expected = before.clone();
    expected["owner"]["events"] = after["owner"]["events"].clone();
    assert!(
        *after == expected,
        "Book reads and bookmark actions must preserve both accounts' full library, provenance, private edits, playlists, likes, progress and explicit download absence"
    );
}
