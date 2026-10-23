use super::playlist_fixture;
use super::provider_fixture::ProviderServer;
use super::support::{app, call};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{ImportProviderPlaylist, PlaylistImportReceipt};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn a_dropped_import_response_replays_without_upstream_or_resurrecting_a_deleted_copy() {
    let (_directory, app) = app().await;
    let credentials = json!({"username": "alice", "password": "playlist replay test password"});
    let (status, session) = call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let token = session["token"].as_str().unwrap();
    let provider = playlist_fixture::connect(&app, token).await;
    let request = ImportProviderPlaylist {
        request_id: Uuid::new_v4(),
        installation_revision: provider.revision,
        playlist_reference: "https://provider.invalid/private-lost-response-selection".into(),
    };
    let body = serde_json::to_value(&request).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&provider.import_path)
                .header("content-type", "application/json")
                .header("x-hito-request", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);

    let before = public_state(&app, token).await;
    assert_eq!(before["playlists"].as_array().unwrap().len(), 1);
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["catalog"]["items"][0]["available"], false);
    assert_eq!(before["feed"]["operations"].as_array().unwrap().len(), 1);
    assert_eq!(
        before["feed"]["operations"][0]["publication"]["kind"],
        "playlist_imported"
    );
    assert_eq!(before["feed"]["has_more"], false);
    let playlist = before["playlists"][0]["id"].as_str().unwrap();
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let detail = get(&app, token, &detail_path).await;
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(detail["playlist"]["kind"], "local");
    assert_eq!(
        detail["origin"]["attribution"]["label"],
        "Curated by the fixture owner"
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| json!([
                entry["position"],
                entry["origin"]["kind"],
                entry["recording_id"].is_null()
            ]))
            .collect::<Vec<_>>(),
        vec![
            json!([0, "recording", false]),
            json!([1, "unavailable", true]),
            json!([2, "unknown", true]),
            json!([3, "recording", false])
        ]
    );
    assert_eq!(entries[0]["recording_id"], entries[3]["recording_id"]);
    assert!(entries.iter().all(|entry| entry["available"] == false));
    assert_eq!(
        entries
            .iter()
            .map(|entry| Uuid::parse_str(entry["entry_id"].as_str().unwrap()).unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    let recording = entries[0]["recording_id"].as_str().unwrap();
    let source_path = format!("/api/v1/recordings/{recording}/sources");
    assert_eq!(get(&app, token, &source_path).await, json!([]));
    let expected_receipt = serde_json::to_value(PlaylistImportReceipt {
        request_id: request.request_id,
        playlist_id: Uuid::parse_str(playlist).unwrap(),
    })
    .unwrap();
    assert_eq!(
        provider
            .server
            .requests("v1/playlists/detail")
            .await
            .into_iter()
            .map(|(_, body)| body)
            .collect::<Vec<_>>(),
        vec![
            json!({"kind": "reference", "reference": request.playlist_reference}),
            json!({"kind": "canonical", "playlist_id": "playlist/retained"}),
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
        vec![json!({
            "playlist_id": "playlist/retained",
            "snapshot": "snapshot-one",
            "cursor": null,
            "limit": 100,
        })]
    );
    let observed = provider_requests(&provider.server).await;
    assert_eq!(observed, [2, 1, 0, 0, 0, 0]);
    provider.server.offline().await;

    let (status, receipt) = call(
        &app,
        "POST",
        &provider.import_path,
        body.clone(),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt, expected_receipt);
    assert_eq!(get(&app, token, &detail_path).await, detail);
    assert_eq!(public_state(&app, token).await, before);
    assert_eq!(get(&app, token, &source_path).await, json!([]));
    assert_eq!(provider_requests(&provider.server).await, observed);

    let deletion = Uuid::new_v4();
    let (status, deleted) = call(
        &app,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": "http-playlist-replay",
            "operations": [{"id": deletion, "action": {
                "kind": "playlist_delete", "playlist_id": playlist
            }}]
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted, json!({"acknowledged": [deletion]}));
    let after_deletion = public_state(&app, token).await;
    assert_eq!(after_deletion["playlists"], json!([]));
    assert_eq!(after_deletion["catalog"], before["catalog"]);
    assert_eq!(
        after_deletion["feed"]["operations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        after_deletion["feed"]["operations"][0],
        before["feed"]["operations"][0]
    );
    assert_eq!(
        after_deletion["feed"]["operations"][1]["operation"]["id"],
        deletion.to_string()
    );
    assert_eq!(
        call(&app, "GET", &detail_path, Value::Null, Some(token))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, receipt) = call(
        &app,
        "POST",
        &provider.import_path,
        body.clone(),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt, expected_receipt);
    assert_eq!(public_state(&app, token).await, after_deletion);
    assert_eq!(
        call(&app, "GET", &detail_path, Value::Null, Some(token))
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    for (field, value) in [
        (
            "playlist_reference",
            json!("https://provider.invalid/changed-selection"),
        ),
        ("installation_revision", json!(provider.revision + 1)),
    ] {
        let mut changed = body.clone();
        changed[field] = value;
        let (status, error) = call(&app, "POST", &provider.import_path, changed, Some(token)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(error.get("error").and_then(Value::as_str).is_some());
        assert!(error.get("playlist_id").is_none());
        assert_eq!(public_state(&app, token).await, after_deletion);
        assert_eq!(provider_requests(&provider.server).await, observed);
    }

    let bob_credentials = json!({"username": "bob", "password": "other playlist replay password"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            bob_credentials.clone(),
            Some(token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, bob) = call(&app, "POST", "/api/v1/login", bob_credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let bob_token = bob["token"].as_str().unwrap();
    let bob_before = public_state(&app, bob_token).await;
    let (status, error) = call(
        &app,
        "POST",
        &provider.import_path,
        body.clone(),
        Some(bob_token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(error.get("playlist_id").is_none());
    assert!(!error.to_string().contains(playlist));
    assert_eq!(public_state(&app, bob_token).await, bob_before);
    assert_eq!(public_state(&app, token).await, after_deletion);
    assert_eq!(
        call(&app, "POST", "/api/v1/logout", Value::Null, Some(token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (status, error) = call(&app, "POST", &provider.import_path, body, Some(token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(error.get("playlist_id").is_none());
    assert!(!error.to_string().contains(playlist));
    let (status, renewed) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        public_state(&app, renewed["token"].as_str().unwrap()).await,
        after_deletion
    );
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
