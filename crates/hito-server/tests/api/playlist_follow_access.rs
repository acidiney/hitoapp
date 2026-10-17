use super::{
    playlist_fixture,
    provider_fixture::ProviderServer,
    support::{app, call},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn follow_receipts_status_and_refresh_work_are_private_to_the_session_account() {
    let (_directory, app) = app().await;
    let credentials = json!({"username": "owner", "password": "owned follow access password"});
    let (status, session) = call(&app, "POST", "/api/v1/setup", credentials.clone(), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let token = session["token"].as_str().unwrap();
    let other_credentials =
        json!({"username": "other", "password": "other follow access password"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            other_credentials.clone(),
            Some(token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, other_session) =
        call(&app, "POST", "/api/v1/login", other_credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other_token = other_session["token"].as_str().unwrap();
    let provider = playlist_fixture::connect(&app, token).await;
    let follow_path = format!(
        "/api/v1/plugins/{}/playlists/follow",
        provider.installation_id
    );
    let body = json!({
        "request_id": Uuid::new_v4(),
        "installation_revision": provider.revision,
        "playlist_reference": "https://provider.invalid/playlists/private-follow",
        "interval": "daily"
    });
    let (status, receipt) = call(&app, "POST", &follow_path, body.clone(), Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    let playlist = receipt["playlist_id"].as_str().unwrap();
    Uuid::parse_str(playlist).unwrap();
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let status_path = format!("{detail_path}/follow");
    let refresh_path = format!("{status_path}/refresh");
    let detail = get(&app, token, &detail_path).await;
    let ready = get(&app, token, &status_path).await;
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["interval"], "daily");
    assert_eq!(
        ready["next_refresh_at"].as_i64().unwrap() - ready["last_refreshed_at"].as_i64().unwrap(),
        86400
    );
    let owner_before = public_state(&app, token).await;
    let other_before = public_state(&app, other_token).await;
    assert_eq!(owner_before["playlists"].as_array().unwrap().len(), 1);
    assert_eq!(other_before["playlists"], json!([]));
    assert_eq!(other_before["catalog"]["total"], 0);
    let observed = provider_requests(&provider.server).await;
    assert_eq!(observed, [2, 1, 0, 0, 0, 0]);

    let (status, replay) = call(&app, "POST", &follow_path, body.clone(), Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, receipt);
    for (field, value) in [
        ("interval", json!("hourly")),
        (
            "playlist_reference",
            json!("https://provider.invalid/playlists/changed-follow"),
        ),
    ] {
        let mut changed = body.clone();
        changed[field] = value;
        let (status, error) = call(&app, "POST", &follow_path, changed, Some(token)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(error.get("error").and_then(Value::as_str).is_some());
        assert!(error.get("playlist_id").is_none());
    }
    let requests = [
        ("GET", status_path.as_str(), Value::Null),
        (
            "POST",
            refresh_path.as_str(),
            json!({"request_id": Uuid::new_v4()}),
        ),
        ("POST", follow_path.as_str(), body),
    ];
    for (access_token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other_token), StatusCode::NOT_FOUND),
    ] {
        for (method, path, body) in &requests {
            let (status, error) = call(&app, method, path, body.clone(), access_token).await;
            assert_eq!(status, expected);
            assert!(error.get("error").and_then(Value::as_str).is_some());
            assert!(error.get("playlist_id").is_none());
            assert!(error.get("job_id").is_none());
            assert!(!error.to_string().contains(playlist));
            assert!(!error.to_string().contains(&provider.installation_id));
        }
    }
    assert_eq!(public_state(&app, token).await, owner_before);
    assert_eq!(public_state(&app, other_token).await, other_before);
    assert_eq!(get(&app, token, &status_path).await, ready);
    assert_eq!(get(&app, token, &detail_path).await, detail);
    assert_eq!(provider_requests(&provider.server).await, observed);

    assert_eq!(
        call(&app, "POST", "/api/v1/logout", Value::Null, Some(token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for (method, path, body) in &requests {
        let (status, error) = call(&app, method, path, body.clone(), Some(token)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(error.get("playlist_id").is_none());
        assert!(error.get("job_id").is_none());
        assert!(!error.to_string().contains(playlist));
        assert!(!error.to_string().contains(&provider.installation_id));
    }
    let (status, renewed) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let renewed_token = renewed["token"].as_str().unwrap();
    assert_eq!(public_state(&app, renewed_token).await, owner_before);
    assert_eq!(public_state(&app, other_token).await, other_before);
    assert_eq!(get(&app, renewed_token, &status_path).await, ready);
    assert_eq!(get(&app, renewed_token, &detail_path).await, detail);
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
