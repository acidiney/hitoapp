use crate::audiobooks_fixture::{Library, Response};
use axum::http::StatusCode;
use hito_core::Action;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn authenticated_audiobook_pages_retain_private_books_chapter_occurrences_and_bookmarks() {
    let library = Library::prepare().await;
    let before = library.state().await;
    let user = library.owner.get(&library.app, "/api/v1/me").await;
    let playlist = &before["owner"]["details"][0];
    let marker = Uuid::new_v4();
    let accepted = library
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
                    position_ms: 350,
                    label: "Private page return point".into(),
                },
            ],
        )
        .await;
    let shared = json!({
        "errors": {}, "native_playback": false, "user": user,
        "libraries": before["owner"]["libraries"], "playlists": before["owner"]["playlists"],
    });
    let index = library.owner.page(&library.app, "/audiobooks").await;
    assert_page(&index, "AudiobookIndexPage", "/audiobooks");
    let mut expected_index = shared.clone();
    expected_index["books"] = json!([playlist["playlist"]]);
    assert!(
        index.body["props"] == expected_index,
        "The Inertia index must contain only the authenticated owner's grouped live book and exact authorized shared props"
    );

    let path = format!("/audiobooks/{}", library.book);
    let detail = library.owner.page(&library.app, &path).await;
    assert_page(&detail, "AudiobookPage", &path);
    let mut expected_detail = shared;
    expected_detail["detail"] = json!({
        "playlist": playlist["playlist"], "chapters": playlist["entries"],
        "bookmarks": [{"id": marker, "audiobook_id": library.book, "entry_id": library.entries[2],
            "recording_id": library.recording, "position_ms": 350, "label": "Private page return point"}],
    });
    assert!(
        detail.body["props"] == expected_detail,
        "The private book page must retain every exact ordered duplicate chapter occurrence and the bookmark's original entry/recording anchor"
    );
    let after = library.state().await;
    let prior_events = before["owner"]["events"]["operations"].as_array().unwrap();
    let events = after["owner"]["events"]["operations"].as_array().unwrap();
    assert!(
        events.len() == prior_events.len() + accepted.len()
            && events[..prior_events.len()] == prior_events[..],
        "The two explicit book actions must retain the accepted prefix without page-read events"
    );
    for (event, operation) in events[prior_events.len()..].iter().zip(&accepted) {
        assert!(
            event["operation"] == *operation && event["device_id"] == "owned-audiobook-http-client",
            "The page adapters must retain exact original sync operation payloads and client identity"
        );
    }
    let mut expected = before;
    expected["owner"]["events"] = after["owner"]["events"].clone();
    assert!(
        after == expected,
        "Rendering book pages must preserve both accounts' complete library/private state, sources, original playlists, likes, progress and explicit download absence"
    );
    library.assert_original_unchanged();
}

fn assert_page(response: &Response, component: &str, path: &str) {
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["x-inertia"], "true");
    assert_eq!(response.headers["vary"], "X-Inertia");
    assert_eq!(response.headers["cache-control"], "private, no-store");
    assert!(
        response.body["component"] == component && response.body["url"] == path,
        "The real Inertia adapter must retain its exact component and requested resource route"
    );
    assert!(
        response.body["encryptHistory"] == true && response.body["clearHistory"] == false,
        "Authenticated private book pages must retain encrypted account-owned history"
    );
}
