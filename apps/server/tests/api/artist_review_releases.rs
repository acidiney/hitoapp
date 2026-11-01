use super::artist_review_queue::queued;
use super::support::call;
use axum::http::StatusCode;
use hito_core::test_support::musicbrainz::Reply;
use serde_json::{Value, json};

const NOVA: &str = "44444444-4444-4444-8444-444444444444";

#[tokio::test]
async fn a_candidate_lists_three_release_titles_through_one_authenticated_bounded_request() {
    let (_dir, app, session, musicbrainz, _origin, recordings) = queued().await;
    let token = Some(session.as_str());
    musicbrainz.release_groups_of(
        NOVA,
        None,
        Reply::ReleaseGroupTitles(
            [
                "Morning Light",
                "Second Wind",
                "Harbor Lights",
                "Quiet Rooms",
            ]
            .map(String::from)
            .to_vec(),
        ),
    );
    let review = format!("/api/v1/recordings/{}/artist-review", recordings[0]);
    let (status, context) = call(
        &app,
        "POST",
        &format!("{review}/context"),
        json!({"provider": {"kind": "builtin_musicbrainz"}}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let selection = json!({"context": context, "provider_artist_id": NOVA});
    let path = format!("{review}/releases");

    assert_eq!(
        call(&app, "POST", &path, selection.clone(), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let before = musicbrainz.requests().len();
    let (status, answer) = call(&app, "POST", &path, selection.clone(), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        answer,
        json!({"status": "available", "context": context,
            "titles": ["Morning Light", "Second Wind", "Harbor Lights"]})
    );
    let requests = musicbrainz.requests();
    assert_eq!(requests.len(), before + 1);
    assert!(requests[before].contains("limit=3"));

    let other = format!(
        "/api/v1/recordings/{}/artist-review/releases",
        recordings[1]
    );
    let (status, _) = call(&app, "POST", &other, selection.clone(), token).await;
    assert!(status.is_client_error());
    let (status, _) = call(
        &app,
        "POST",
        &path,
        json!({"context": context, "provider_artist_id": "not-an-id"}),
        token,
    )
    .await;
    assert!(status.is_client_error());
    let (status, _) = call(
        &app,
        "POST",
        &path,
        json!({"context": context, "provider_artist_id": NOVA, "extra": Value::Null}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(musicbrainz.requests().len(), before + 1);
}
