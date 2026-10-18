use super::{
    fixture::{copy_identity, listening_state},
    private_copy, reject, request,
};
use crate::support::call;
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn accepted_receipts_replay_independently_of_new_connection_approval() {
    let f = private_copy().await;
    let accepted_body = request(&f);
    let before = f.state().await;
    let (status, accepted_bytes) = f.refresh(&accepted_body).await;
    assert_eq!(status, StatusCode::OK);
    let accepted: Value = serde_json::from_slice(&accepted_bytes).unwrap();
    assert_eq!(accepted["request_id"], accepted_body["request_id"]);
    assert_eq!(accepted["playlist_id"], f.playlist);
    assert_eq!(accepted["updated_recordings"], 0);
    assert_eq!(accepted["unchanged_recordings"], 1);
    assert_eq!(accepted["unavailable_recordings"], 0);
    assert_eq!(f.state().await, before);
    let mut conflicting = accepted_body.clone();
    conflicting["installation_revision"] = json!(f.provider.revision + 1);
    let receipt_conflict = "A playlist metadata request ID was reused with different contents";
    reject(
        &f,
        conflicting.clone(),
        Some(&f.token),
        StatusCode::CONFLICT,
        receipt_conflict,
    )
    .await;
    let mut unaccepted = request(&f);
    unaccepted["installation_revision"] = conflicting["installation_revision"].clone();
    reject(
        &f,
        unaccepted,
        Some(&f.token),
        StatusCode::CONFLICT,
        "This playlist connection changed",
    )
    .await;

    f.provider
        .server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.http-playlist-fixture", "name": "Reviewed metadata fixture",
                "protocol_version": 1, "capabilities": ["playlists", "search"]
            }),
        )
        .await;
    let path = format!("/api/v1/plugins/{}/refresh", f.provider.installation_id);
    let settings_before = f.get("/api/v1/plugins").await;
    let observed = f.provider_requests().await;
    let (status, review) = call(
        &f.app,
        "POST",
        &path,
        json!({"revision": f.provider.revision}),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["kind"], "review_required");
    assert!(review["digest"].is_string());
    assert_eq!(f.get("/api/v1/plugins").await, settings_before);
    assert_eq!(f.state().await, before);
    let (status, approved) = call(
        &f.app,
        "POST",
        &path,
        json!({
            "revision": f.provider.revision, "accept_manifest_digest": review["digest"]
        }),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(approved["kind"], "updated");
    assert_eq!(approved["installation"]["manifest"], review["manifest"]);
    let current_revision = approved["installation"]["revision"].as_i64().unwrap();
    assert_eq!(current_revision, f.provider.revision + 1);
    let mut expected = observed;
    expected[0] += 2;
    assert_eq!(f.provider_requests().await, expected);
    assert_eq!(f.state().await, before);
    let settings = f.get("/api/v1/plugins").await;

    f.publish_verified_album().await;
    let mut current_body = request(&f);
    reject(
        &f,
        current_body.clone(),
        Some(&f.token),
        StatusCode::CONFLICT,
        "This playlist connection changed",
    )
    .await;
    reject(
        &f,
        conflicting.clone(),
        Some(&f.token),
        StatusCode::CONFLICT,
        receipt_conflict,
    )
    .await;
    current_body["installation_revision"] = json!(current_revision);
    let (status, current_bytes) = f.refresh(&current_body).await;
    assert_eq!(status, StatusCode::OK);
    let current: Value = serde_json::from_slice(&current_bytes).unwrap();
    assert_eq!(current["request_id"], current_body["request_id"]);
    assert_eq!(current["playlist_id"], f.playlist);
    assert_eq!(current["updated_recordings"], 1);
    assert_eq!(current["unchanged_recordings"], 0);
    assert_eq!(current["unavailable_recordings"], 0);
    let after = f.state().await;
    assert_eq!(
        copy_identity(&after["copy"]),
        copy_identity(&before["copy"])
    );
    assert_eq!(
        listening_state(&after["activity"]),
        listening_state(&before["activity"])
    );
    assert_eq!(after["catalog"]["total"], 1);
    assert_eq!(after["albums"]["total"], 1);
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
    let previous_events = before["events"]["operations"].as_array().unwrap();
    let events = after["events"]["operations"].as_array().unwrap();
    assert_eq!(events.len(), previous_events.len() + 1);
    assert_eq!(&events[..previous_events.len()], previous_events.as_slice());
    let publication = &events.last().unwrap()["publication"];
    assert_eq!(publication["kind"], "playlist_metadata_refreshed");
    assert_eq!(
        publication["origin"]["installation_revision"],
        current_revision
    );
    expected[1] += 2;
    expected[2] += 1;
    assert_eq!(f.provider_requests().await, expected);
    assert_eq!(f.get("/api/v1/plugins").await, settings);

    f.provider.server.offline().await;
    reject(
        &f,
        conflicting,
        Some(&f.token),
        StatusCode::CONFLICT,
        receipt_conflict,
    )
    .await;
    for (body, receipt) in [
        (&accepted_body, &accepted_bytes),
        (&current_body, &current_bytes),
    ] {
        let (status, replay) = f.refresh(body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&replay, receipt);
        assert_eq!(f.state().await, after);
        assert_eq!(f.provider_requests().await, expected);
    }
    assert_eq!(f.get("/api/v1/plugins").await, settings);
}
