use crate::audiobooks_fixture::{Library, Response};
use axum::http::StatusCode;
use hito_core::{Action, Error};
use serde_json::{Value, json};
use uuid::Uuid;

fn chapter(library: &Library, entry: usize) -> Value {
    json!({
        "audiobook_id": library.book,
        "entry_id": library.entries[entry],
        "recording_id": library.recording,
    })
}

fn assert_denied(response: Response, status: StatusCode, error: Error) {
    assert_eq!(response.status, status);
    assert_eq!(response.body, json!({"error": error.to_string()}));
}

async fn grouped() -> Library {
    let library = Library::prepare().await;
    library
        .owner
        .sync(
            &library.app,
            vec![Action::AudiobookCreate {
                playlist_id: library.book,
            }],
        )
        .await;
    library
}

#[tokio::test]
async fn a_remote_player_checks_its_own_chapters_and_nobody_else_can() {
    let library = grouped().await;
    let before = library.retained_state().await;
    let path = "/api/v1/audiobooks/chapters/check";
    let chapters = json!({"chapters": [chapter(&library, 0), chapter(&library, 2)]});

    let own = library
        .owner
        .request(&library.app, "POST", path, chapters.clone())
        .await;
    assert_eq!(own.status, StatusCode::NO_CONTENT);
    assert_eq!(own.headers["cache-control"], "private, no-store");

    let peer = library
        .peer
        .request(&library.app, "POST", path, chapters.clone())
        .await;
    assert_denied(peer, StatusCode::NOT_FOUND, Error::NotFound);

    let (status, _) = crate::support::call(&library.app, "POST", path, chapters, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let unknown = json!({"chapters": [{
        "audiobook_id": library.book,
        "entry_id": Uuid::new_v4(),
        "recording_id": library.recording,
    }]});
    let unknown = library
        .owner
        .request(&library.app, "POST", path, unknown)
        .await;
    assert_denied(unknown, StatusCode::NOT_FOUND, Error::NotFound);

    let too_many: Vec<Value> = (0..1001).map(|_| chapter(&library, 0)).collect();
    let too_many = library
        .owner
        .request(&library.app, "POST", path, json!({"chapters": too_many}))
        .await;
    assert_eq!(too_many.status, StatusCode::UNPROCESSABLE_ENTITY);

    assert!(
        library.retained_state().await == before,
        "Checking chapters must never change either account"
    );
}

#[tokio::test]
async fn a_remote_player_is_admitted_to_its_own_book_and_only_its_own() {
    let library = grouped().await;
    let before = library.retained_state().await;
    let path = format!("/api/v1/audiobooks/{}/admission", library.book);
    let admission = json!({
        "target": chapter(&library, 1),
        "chapters": [chapter(&library, 0), chapter(&library, 1), chapter(&library, 2)],
    });

    let own = library
        .owner
        .request(&library.app, "POST", &path, admission.clone())
        .await;
    assert_eq!(own.status, StatusCode::NO_CONTENT);
    assert_eq!(own.headers["cache-control"], "private, no-store");

    let peer = library
        .peer
        .request(&library.app, "POST", &path, admission.clone())
        .await;
    assert_denied(peer, StatusCode::NOT_FOUND, Error::NotFound);

    let other_book = library
        .owner
        .request(
            &library.app,
            "POST",
            &format!("/api/v1/audiobooks/{}/admission", Uuid::new_v4()),
            admission,
        )
        .await;
    assert_eq!(other_book.status, StatusCode::UNPROCESSABLE_ENTITY);

    let target_outside = library
        .owner
        .request(
            &library.app,
            "POST",
            &path,
            json!({"target": chapter(&library, 1), "chapters": [chapter(&library, 0)]}),
        )
        .await;
    assert_eq!(target_outside.status, StatusCode::UNPROCESSABLE_ENTITY);

    assert!(
        library.retained_state().await == before,
        "Admission must never change either account"
    );
}
