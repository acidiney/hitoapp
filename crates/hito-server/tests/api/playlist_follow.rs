use super::{
    playlist_fixture,
    provider_fixture::ProviderServer,
    support::{admin, app, call},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn a_provider_playlist_can_be_followed_and_its_refresh_queued_idempotently() {
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let provider = playlist_fixture::connect(&app, &token).await;
    let follow_path = format!(
        "/api/v1/plugins/{}/playlists/follow",
        provider.installation_id
    );
    let request_id = Uuid::new_v4();
    let reference = "https://provider.invalid/playlists/followed-reference";
    let (status, receipt) = call(
        &app,
        "POST",
        &follow_path,
        json!({
            "request_id": request_id,
            "installation_revision": provider.revision,
            "playlist_reference": reference,
            "interval": "hourly"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["request_id"], request_id.to_string());
    assert_eq!(
        receipt
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["playlist_id", "request_id"]
    );
    let playlist = receipt["playlist_id"].as_str().unwrap();
    Uuid::parse_str(playlist).unwrap();
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let detail = get(&app, &token, &detail_path).await;
    assert_eq!(detail["playlist"]["kind"], "followed");
    assert_eq!(detail["playlist"]["name"], "Provider selection");
    assert_eq!(
        detail["origin"]["attribution"]["label"],
        "Curated by the fixture owner"
    );
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| json!([
                entry["position"],
                entry["title"],
                entry["origin"]["kind"],
                entry["origin"]["occurrence_id"]
            ]))
            .collect::<Vec<_>>(),
        [
            json!([0, "Known recording", "recording", "first"]),
            json!([1, "Withdrawn recording", "unavailable", "withdrawn"]),
            json!([2, "Unknown provider item", "unknown", "unknown"]),
            json!([3, "Known recording", "recording", "duplicate"])
        ]
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| Uuid::parse_str(entry["entry_id"].as_str().unwrap()).unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    assert_eq!(entries[0]["recording_id"], entries[3]["recording_id"]);
    let recording = entries[0]["recording_id"].as_str().unwrap();
    Uuid::parse_str(recording).unwrap();
    assert!(entries[1]["recording_id"].is_null());
    assert!(entries[2]["recording_id"].is_null());
    assert!(entries.iter().all(|entry| entry["available"] == false));
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    assert_eq!(get(&app, &token, &sources_path).await, json!([]));

    let status_path = format!("{detail_path}/follow");
    let ready = get(&app, &token, &status_path).await;
    assert_eq!(ready["playlist_id"], playlist);
    assert_eq!(ready["interval"], "hourly");
    assert_eq!(ready["snapshot"], "snapshot-one");
    assert_eq!(ready["status"], "ready");
    assert!(ready["error"].is_null());
    assert_eq!(
        ready["next_refresh_at"].as_i64().unwrap() - ready["last_refreshed_at"].as_i64().unwrap(),
        3600
    );
    assert_eq!(
        provider
            .server
            .requests("v1/playlists/detail")
            .await
            .into_iter()
            .map(|(_, body)| body)
            .collect::<Vec<_>>(),
        [
            json!({"kind": "reference", "reference": reference}),
            json!({"kind": "canonical", "playlist_id": "playlist/retained"})
        ]
    );
    assert_eq!(
        provider
            .server
            .requests("v1/playlists/items")
            .await
            .into_iter()
            .map(|(_, body)| body)
            .collect::<Vec<_>>(),
        [json!({
            "playlist_id": "playlist/retained", "snapshot": "snapshot-one",
            "cursor": null, "limit": 100
        })]
    );
    let before = provider_requests(&provider.server).await;
    assert_eq!(before, [2, 1, 0, 0, 0, 0]);

    let refresh_path = format!("{status_path}/refresh");
    let refresh_id = Uuid::new_v4();
    let body = json!({"request_id": refresh_id});
    let (status, refresh) = call(&app, "POST", &refresh_path, body.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(refresh["request_id"], refresh_id.to_string());
    assert_eq!(refresh["playlist_id"], playlist);
    Uuid::parse_str(refresh["job_id"].as_str().unwrap()).unwrap();
    let (status, replay) = call(&app, "POST", &refresh_path, body, Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(replay, refresh);
    let mut expected = ready;
    expected["status"] = json!("queued");
    assert_eq!(get(&app, &token, &status_path).await, expected);
    assert_eq!(get(&app, &token, &detail_path).await, detail);
    assert_eq!(get(&app, &token, &sources_path).await, json!([]));
    assert_eq!(provider_requests(&provider.server).await, before);
}

async fn get(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
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
