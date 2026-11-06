use crate::{
    audiobooks_fixture::{Library, Response},
    support::call,
};
use axum::http::StatusCode;
use hito_core::{Action, Error};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn audiobook_reads_enforce_account_scope_and_hide_deleted_backing_playlists() {
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
                    label: "Private access return point".into(),
                },
            ],
        )
        .await;
    let full_before = library.state().await;
    let before = library.retained_state().await;
    let peer_book = full_before["peer"]["details"][0]["playlist"]["id"]
        .as_str()
        .unwrap();
    let ungrouped = full_before["owner"]["details"][1]["playlist"]["id"]
        .as_str()
        .unwrap();
    let detail_path = format!("/api/v1/audiobooks/{}", library.book);
    let own = library.owner.get(&library.app, &detail_path).await;
    assert!(
        own["bookmarks"].as_array().unwrap().len() == 1
            && own["chapters"].as_array().unwrap().len() == 3,
        "The access workflow must begin with an owned live book and private anchored bookmark"
    );
    let peer_recordings = &full_before["peer"]["recordings"]["items"];
    assert!(
        peer_recordings
            .as_array()
            .unwrap()
            .iter()
            .any(|recording| recording["id"] == library.recording.to_string()
                && recording["available"] == true),
        "The second account must retain current access to the same original recording independently of private book authority"
    );

    for path in ["/api/v1/audiobooks", detail_path.as_str()] {
        let (status, body) = call(&library.app, "GET", path, Value::Null, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            body == json!({"error": Error::Unauthorized.to_string()}),
            "Anonymous book reads must return the ordinary typed session rejection"
        );
        let mismatch = library
            .owner
            .get_claiming(&library.app, path, &library.peer)
            .await;
        assert_denied(mismatch, StatusCode::FORBIDDEN, Error::Forbidden);
    }
    for id in [
        peer_book.to_owned(),
        ungrouped.to_owned(),
        Uuid::new_v4().to_string(),
    ] {
        let denied = library
            .owner
            .request(
                &library.app,
                "GET",
                &format!("/api/v1/audiobooks/{id}"),
                Value::Null,
            )
            .await;
        assert_denied(denied, StatusCode::NOT_FOUND, Error::NotFound);
    }
    assert!(
        library.state().await == full_before && library.retained_state().await == before,
        "Anonymous, mismatched, foreign, ungrouped and unknown book reads must preserve both accounts and every accepted event"
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
        "The explicit parent deletion must retain its exact single acknowledgement"
    );
    let list = library
        .owner
        .request(&library.app, "GET", "/api/v1/audiobooks", Value::Null)
        .await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.headers["cache-control"], "private, no-store");
    assert!(
        list.body == json!([]),
        "A deleted owned book must disappear without exposing the peer's private book"
    );
    let deleted = library
        .owner
        .request(&library.app, "GET", &detail_path, Value::Null)
        .await;
    assert_denied(deleted, StatusCode::NOT_FOUND, Error::NotFound);
    let after = library.retained_state().await;
    let prior = before["owner"]["events"]["operations"].as_array().unwrap();
    let current = after["owner"]["events"]["operations"].as_array().unwrap();
    assert!(
        current.len() == prior.len() + 1
            && current[..prior.len()] == prior[..]
            && current.last().unwrap()["operation"] == deletion[0]
            && current.last().unwrap()["device_id"] == "owned-audiobook-http-client",
        "Deleting the backing playlist must append exactly the accepted operation and retain every prior event"
    );
    let mut expected = before.clone();
    expected["owner"]["events"] = after["owner"]["events"].clone();
    expected["owner"]["playlists"]
        .as_array_mut()
        .unwrap()
        .retain(|playlist| playlist["id"] != library.book.to_string());
    expected["owner"]["metadata"]["appears_in"]
        .as_array_mut()
        .unwrap()
        .retain(|playlist| playlist["id"] != library.book.to_string());
    assert!(
        after == expected,
        "Parent deletion and subsequent reads must preserve the peer's complete private state, owner metadata/sources/likes/progress, unrelated duplicate playlist and download absence"
    );
    library.assert_original_unchanged();
}

fn assert_denied(response: Response, status: StatusCode, error: Error) {
    assert_eq!(response.status, status);
    assert_eq!(response.headers["cache-control"], "private, no-store");
    assert!(
        response.body == json!({"error": error.to_string()}),
        "Book read rejection must preserve the ordinary typed error without private identifiers or metadata"
    );
}
