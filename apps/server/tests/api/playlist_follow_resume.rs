use super::{
    playlist_fixture,
    provider_fixture::ProviderServer,
    support::{admin, app, call},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[tokio::test]
async fn an_explicit_reviewed_resume_reauthorizes_the_same_follow_without_rewriting_its_saved_snapshot()
 {
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let provider = playlist_fixture::connect(&app, &token).await;
    let follow_path = format!(
        "/api/v1/plugins/{}/playlists/follow",
        provider.installation_id
    );
    let (status, receipt) = call(
        &app,
        "POST",
        &follow_path,
        json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": provider.revision,
            "playlist_reference": "https://provider.invalid/playlists/resumed-follow",
            "interval": "hourly"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let playlist = receipt["playlist_id"].as_str().unwrap();
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let status_path = format!("{detail_path}/follow");
    let refresh_path = format!("{status_path}/refresh");
    let saved = get(&app, &token, &detail_path).await;
    let ready = get(&app, &token, &status_path).await;
    assert_eq!(saved["playlist"]["kind"], "followed");
    assert_eq!(saved["entries"].as_array().unwrap().len(), 4);
    assert_eq!(ready["status"], "ready");
    assert_eq!(saved["origin"]["installation_id"], provider.installation_id);
    assert_eq!(saved["origin"]["installation_revision"], provider.revision);
    let settings = get(&app, &token, "/api/v1/plugins").await;
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == saved["origin"]["service_id"])
        .unwrap();
    assert_eq!(saved["origin"]["service_revision"], service["revision"]);
    let observed = provider_requests(&provider.server).await;
    assert_eq!(observed, [2, 1, 0, 0, 0, 0]);

    provider
        .server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.http-playlist-fixture",
                "name": "Reviewed HTTP playlist fixture",
                "protocol_version": 1,
                "capabilities": ["playlists", "search"]
            }),
        )
        .await;
    let connection_path = format!("/api/v1/plugins/{}/refresh", provider.installation_id);
    let (status, review) = call(
        &app,
        "POST",
        &connection_path,
        json!({"revision": provider.revision}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["kind"], "review_required");
    assert_eq!(
        review["manifest"]["capabilities"],
        json!(["search", "playlists"])
    );
    assert!(review["digest"].as_str().is_some());
    assert_eq!(get(&app, &token, "/api/v1/plugins").await, settings);
    assert_eq!(get(&app, &token, &status_path).await, ready);
    let (status, accepted) = call(
        &app,
        "POST",
        &connection_path,
        json!({
            "revision": provider.revision,
            "accept_manifest_digest": review["digest"]
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accepted["kind"], "updated");
    assert_eq!(accepted["installation"]["id"], provider.installation_id);
    assert_eq!(accepted["installation"]["service_id"], service["id"]);
    assert_eq!(accepted["installation"]["revision"], provider.revision + 1);
    assert_eq!(accepted["installation"]["manifest"], review["manifest"]);
    let approved_settings = get(&app, &token, "/api/v1/plugins").await;
    let paused = get(&app, &token, &status_path).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["error"], "authority_changed");
    assert_eq!(paused["snapshot"], ready["snapshot"]);
    assert_eq!(get(&app, &token, &detail_path).await, saved);
    let refresh_request = json!({"request_id": Uuid::new_v4()});
    assert_eq!(
        call(
            &app,
            "POST",
            &refresh_path,
            refresh_request.clone(),
            Some(&token)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let before = public_state(&app, &token).await;
    assert_eq!(provider_requests(&provider.server).await, observed);

    let operation_id = Uuid::new_v4();
    let batch = json!({
        "device_id": "http-follow-resume",
        "operations": [{"id": operation_id, "action": {
            "kind": "playlist_follow_resume",
            "playlist_id": playlist,
            "expected_generation": 1,
            "installation_revision": accepted["installation"]["revision"],
            "service_revision": service["revision"]
        }}]
    });
    let (status, acknowledgement) =
        call(&app, "POST", "/api/v1/sync", batch.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(acknowledgement, json!({"acknowledged": [operation_id]}));
    let resumed = get(&app, &token, &status_path).await;
    assert_eq!(resumed["generation"], 2);
    assert_eq!(resumed["status"], "ready");
    assert!(resumed["error"].is_null());
    assert_eq!(resumed["interval"], ready["interval"]);
    assert_eq!(resumed["snapshot"], ready["snapshot"]);
    assert_eq!(resumed["last_refreshed_at"], ready["last_refreshed_at"]);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(resumed["next_refresh_at"].as_i64().unwrap() <= now);
    assert_eq!(get(&app, &token, &detail_path).await, saved);
    assert_eq!(
        get(&app, &token, "/api/v1/plugins").await,
        approved_settings
    );
    let after = public_state(&app, &token).await;
    assert_eq!(after["playlists"], before["playlists"]);
    assert_eq!(after["catalog"], before["catalog"]);
    assert_eq!(after["activity"], before["activity"]);
    let previous_events = before["feed"]["operations"].as_array().unwrap();
    let events = after["feed"]["operations"].as_array().unwrap();
    assert_eq!(events.len(), previous_events.len() + 1);
    assert_eq!(&events[..previous_events.len()], previous_events.as_slice());
    let event = events.last().unwrap();
    assert_eq!(event["operation"], batch["operations"][0]);
    assert_eq!(event["device_id"], "http-follow-resume");
    assert!(event.get("publication").is_none());
    assert_eq!(after["feed"]["has_more"], false);
    assert_eq!(provider_requests(&provider.server).await, observed);

    let (status, replay) = call(&app, "POST", "/api/v1/sync", batch, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, acknowledgement);
    assert_eq!(get(&app, &token, &status_path).await, resumed);
    assert_eq!(get(&app, &token, &detail_path).await, saved);
    assert_eq!(public_state(&app, &token).await, after);
    assert_eq!(provider_requests(&provider.server).await, observed);
    let (status, queued) = call(&app, "POST", &refresh_path, refresh_request, Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(queued["playlist_id"], playlist);
    Uuid::parse_str(queued["job_id"].as_str().unwrap()).unwrap();
    let queued_status = get(&app, &token, &status_path).await;
    assert_eq!(queued_status["generation"], 2);
    assert_eq!(queued_status["status"], "queued");
    assert_eq!(get(&app, &token, &detail_path).await, saved);
    assert_eq!(public_state(&app, &token).await, after);
    assert_eq!(provider_requests(&provider.server).await, observed);
}

async fn get(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn public_state(app: &Router, token: &str) -> Value {
    json!({
        "playlists": get(app, token, "/api/v1/playlists").await,
        "catalog": get(app, token, "/api/v1/recordings").await,
        "feed": get(app, token, "/api/v1/sync?cursor=0").await,
        "activity": get(app, token, "/api/v1/activity").await
    })
}

async fn provider_requests(server: &ProviderServer) -> [usize; 6] {
    [
        server.requests("v1/playlists/detail").await.len(),
        server.requests("v1/playlists/items").await.len(),
        server.requests("v1/search").await.len(),
        server.requests("v1/recordings/detail").await.len(),
        server.requests("v1/sources").await.len(),
        server.requests("v1/audio").await.len(),
    ]
}
