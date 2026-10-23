use super::{
    fixture::{copy_identity, listening_state},
    private_copy, request,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::task::JoinSet;
use tower::ServiceExt;

#[tokio::test]
async fn an_uncoded_busy_conflict_can_precede_the_same_requests_accepted_receipt() {
    let f = private_copy().await;
    f.publish_verified_album().await;
    let before = f.state().await;
    let body = request(&f);
    let gate = f.provider.server.hold("v1/playlists/items").await;
    let mut pending = JoinSet::new();
    let (app, token, playlist, submitted) = (
        f.app.clone(),
        f.token.clone(),
        f.playlist.clone(),
        body.clone(),
    );
    pending.spawn(async move {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/playlists/{playlist}/metadata/refresh"))
                    .header("content-type", "application/json")
                    .header("x-hito-request", "1")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(submitted.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, bytes.to_vec())
    });
    tokio::time::timeout(Duration::from_secs(5), gate.entered())
        .await
        .expect("The original HTTP snapshot must be held before its retry");
    let held_requests = f.provider_requests().await;
    for attempted in [body.clone(), request(&f)] {
        let (status, bytes) = tokio::time::timeout(Duration::from_secs(1), f.refresh(&attempted))
            .await
            .expect("Busy HTTP work must fail promptly without another provider request");
        assert_eq!(status, StatusCode::CONFLICT);
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            error,
            json!({"error": "Provider requests for this account are busy. Try again."})
        );
        assert!(error.get("code").is_none());
        assert_eq!(f.state().await, before);
        assert_eq!(f.provider_requests().await, held_requests);
    }
    gate.release();
    let (status, accepted_bytes) =
        tokio::time::timeout(Duration::from_secs(5), pending.join_next())
            .await
            .expect("The released HTTP owner must finish")
            .expect("The HTTP owner must stay joined")
            .expect("The HTTP owner must not panic");
    assert!(pending.is_empty());
    assert_eq!(status, StatusCode::OK);
    let accepted: Value = serde_json::from_slice(&accepted_bytes).unwrap();
    assert_eq!(
        accepted,
        json!({
            "request_id": body["request_id"], "playlist_id": f.playlist,
            "updated_recordings": 1, "unchanged_recordings": 0, "unavailable_recordings": 0
        })
    );
    let after = f.state().await;
    assert_eq!(
        copy_identity(&after["copy"]),
        copy_identity(&before["copy"])
    );
    assert_eq!(
        listening_state(&after["activity"]),
        listening_state(&before["activity"])
    );
    assert_eq!(after["sources"], before["sources"]);
    assert_eq!(after["source_choice"], before["source_choice"]);
    for field in ["title", "artist"] {
        for retained in ["personal", "operation_id", "value"] {
            assert_eq!(
                after["metadata"][field][retained],
                before["metadata"][field][retained]
            );
        }
    }
    let prior = before["events"]["operations"].as_array().unwrap();
    let events = after["events"]["operations"].as_array().unwrap();
    assert_eq!(events.len(), prior.len() + 1);
    assert_eq!(&events[..prior.len()], prior.as_slice());
    assert_eq!(
        events.last().unwrap()["publication"]["kind"],
        "playlist_metadata_refreshed"
    );
    let mut expected_requests = held_requests;
    expected_requests[1] += 1;
    assert_eq!(f.provider_requests().await, expected_requests);
    f.provider.server.offline().await;
    let (status, replayed_bytes) = f.refresh(&body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replayed_bytes, accepted_bytes);
    assert_eq!(f.state().await, after);
    assert_eq!(f.provider_requests().await, expected_requests);
    assert_eq!(
        crate::support::call(
            &f.app,
            "GET",
            &format!("/api/v1/recordings/{}/download", f.recording),
            Value::Null,
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
