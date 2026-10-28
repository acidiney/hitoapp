use crate::audiobooks_fixture::{Library, Response, access_page};
use axum::http::StatusCode;
use hito_core::{Action, Error};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn audiobook_pages_authorize_the_book_before_selecting_partial_shared_props() {
    let library = Library::prepare().await;
    library
        .owner
        .sync(
            &library.app,
            vec![
                Action::AudiobookCreate {
                    playlist_id: library.book,
                },
                Action::AudiobookBookmarkCreate {
                    audiobook_id: library.book,
                    bookmark_id: Uuid::new_v4(),
                    entry_id: library.entries[2],
                    recording_id: library.recording,
                    position_ms: 250,
                    label: "Private partial page return point".into(),
                },
            ],
        )
        .await;
    let before = library.state().await;
    let retained = library.retained_state().await;
    let user = library.owner.get(&library.app, "/api/v1/me").await;
    let path = format!("/audiobooks/{}", library.book);
    let detail_path = format!("/api/v1/audiobooks/{}", library.book);
    let detail = library.owner.get(&library.app, &detail_path).await;
    let peer_book = before["peer"]["details"][0]["playlist"]["id"]
        .as_str()
        .unwrap();
    let ungrouped = before["owner"]["details"][1]["playlist"]["id"]
        .as_str()
        .unwrap();
    assert!(
        before["peer"]["recordings"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|recording| recording["id"] == library.recording.to_string()
                && recording["available"] == true),
        "The peer must retain shared-library access to the chapter recording without authority over the owner's private book"
    );

    for target in ["/audiobooks", path.as_str()] {
        let anonymous = access_page(&library.app, target, None, false).await;
        assert_page(&anonymous, "LoginPage", target, true);
        assert!(
            anonymous.body["props"] == json!({"errors": {}, "needs_setup": false}),
            "Anonymous book navigation must expose only the ordinary login props, without book or account-private data"
        );
    }

    let partial = access_page(&library.app, &path, Some(&library.owner), true).await;
    assert_page(&partial, "AudiobookPage", &path, false);
    assert!(
        partial.body["props"]
            == json!({"errors": {}, "user": user, "playlists": before["owner"]["playlists"]}),
        "An authorized shared-only partial must contain exactly the owner's requested shared props and omit book detail"
    );
    for id in [
        peer_book.to_owned(),
        ungrouped.to_owned(),
        Uuid::new_v4().to_string(),
        "not-a-uuid".to_owned(),
    ] {
        let target = format!("/audiobooks/{id}");
        for shared_only in [false, true] {
            let denied =
                access_page(&library.app, &target, Some(&library.owner), shared_only).await;
            assert_not_found(&denied);
        }
    }
    assert!(
        library.state().await == before
            && library.retained_state().await == retained
            && library.owner.get(&library.app, &detail_path).await == detail,
        "Login, authorized partial and denied page reads must retain both accounts' exact private state, chapter occurrences, sources, progress and event history"
    );

    let deletion = library
        .owner
        .sync(
            &library.app,
            vec![Action::PlaylistDelete {
                playlist_id: library.book,
            }],
        )
        .await;
    assert!(
        deletion.len() == 1,
        "The explicit backing-playlist deletion must acknowledge its exact single operation"
    );
    for shared_only in [false, true] {
        let deleted = access_page(&library.app, &path, Some(&library.owner), shared_only).await;
        assert_not_found(&deleted);
    }
    let index = library.owner.page(&library.app, "/audiobooks").await;
    assert_page(&index, "AudiobookIndexPage", "/audiobooks", false);
    let mut expected_playlists = retained["owner"]["playlists"].clone();
    expected_playlists
        .as_array_mut()
        .unwrap()
        .retain(|playlist| playlist["id"] != library.book.to_string());
    assert!(
        index.body["props"]
            == json!({
                    "errors": {}, "native_playback": false, "user": user,
            "lock": {"pin_enabled": false, "idle_minutes": 0, "locked": false},
                    "libraries": retained["owner"]["libraries"],
                    "playlists": expected_playlists, "books": [],
                    "book_pagination": {"limit": 60, "next_cursor": null, "previous_cursor": null},
                    "grouping_playlists": expected_playlists.as_array().unwrap().iter().filter(|playlist| playlist["kind"] == "local").collect::<Vec<_>>(),
                }),
        "Deleting the backing playlist must remove the book from the owner's exact private index without exposing the peer's book or resurrecting its chapters"
    );

    let after = library.retained_state().await;
    let prior_events = retained["owner"]["events"]["operations"]
        .as_array()
        .unwrap();
    let events = after["owner"]["events"]["operations"].as_array().unwrap();
    assert!(
        events.len() == prior_events.len() + 1
            && events[..prior_events.len()] == prior_events[..]
            && events.last().unwrap()["operation"] == deletion[0]
            && events.last().unwrap()["device_id"] == "owned-audiobook-http-client",
        "The deletion must append exactly its accepted original payload while subsequent full and partial page reads create no events"
    );
    let mut expected = retained;
    expected["owner"]["events"] = after["owner"]["events"].clone();
    expected["owner"]["playlists"] = expected_playlists;
    assert!(
        after == expected,
        "Deleted-book navigation must preserve the peer's complete private state, owner metadata/likes/progress, unrelated duplicate playlist, source identities and explicit download absence"
    );
    library.assert_original_unchanged();
}

fn assert_page(response: &Response, component: &str, path: &str, clear_history: bool) {
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["x-inertia"], "true");
    assert_eq!(response.headers["vary"], "X-Inertia");
    assert_eq!(response.headers["cache-control"], "private, no-store");
    assert!(
        response.body["component"] == component && response.body["url"] == path,
        "The real Inertia response must retain the exact page component and requested route"
    );
    assert!(
        response.body["encryptHistory"] == true && response.body["clearHistory"] == clear_history,
        "Private page navigation must encrypt history and clear it when returning to anonymous login"
    );
}

fn assert_not_found(response: &Response) {
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.headers["cache-control"], "private, no-store");
    assert!(
        response.body == json!({"error": Error::NotFound.to_string()}),
        "Unavailable book pages must return the ordinary typed rejection without shared or book-private props"
    );
}
