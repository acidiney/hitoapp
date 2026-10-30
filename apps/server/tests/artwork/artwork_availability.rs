#[path = "../fixtures/artwork_provider.rs"]
#[allow(dead_code)]
mod provider;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use support::{admin, call};
use uuid::Uuid;

#[tokio::test]
async fn artwork_availability_reads_current_private_provider_eligibility_without_fetching() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    support::write_wav(&music.join("Local.wav"));
    let hito = support::services(directory.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let libraries = get(&app, &token, "/api/v1/libraries").await;
    assert_eq!(
        call(
            &app,
            "POST",
            &format!(
                "/api/v1/libraries/{}/scan",
                libraries[0]["id"].as_str().unwrap()
            ),
            Value::Null,
            Some(&token),
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert!(hito.storage.run_next_job().await.unwrap());
    let local_catalog = get(&app, &token, "/api/v1/recordings").await;
    let local_recording = local_catalog["items"][0]["id"].as_str().unwrap();

    let provider = provider::ArtworkServer::start().await;
    let (status, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned availability fixture", "base_url": provider.url}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let installation_path = format!("/api/v1/plugins/{}", installation["id"].as_str().unwrap());
    let (status, imported) = call(
        &app,
        "POST",
        &format!("{installation_path}/playlists/import"),
        json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/owned-selection"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let playlist = imported["playlist_id"].as_str().unwrap();
    let detail = get(&app, &token, &format!("/api/v1/playlists/{playlist}")).await;
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0]["entry_id"], entries[1]["entry_id"]);
    assert_eq!(entries[0]["recording_id"], entries[1]["recording_id"]);
    let recording = entries[0]["recording_id"].as_str().unwrap();
    let path = format!("/api/v1/recordings/{recording}/artwork/availability");
    let local_path = format!("/api/v1/recordings/{local_recording}/artwork/availability");
    let credentials = json!({"username": "other", "password": "other availability password"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(&token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, other_session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other_token = other_session["token"].as_str().unwrap();
    let before = saved_state(&app, &token, playlist, recording).await;
    let requests = provider.requests().await;

    for _ in 0..3 {
        assert_eq!(get(&app, &token, &path).await, json!({"can_fetch": true}));
    }
    assert_eq!(
        get(&app, &token, &local_path).await,
        json!({"can_fetch": false})
    );
    assert!(
        !hito
            .catalog
            .run_next_artwork_job_until(async {
                tokio::time::sleep(Duration::from_secs(1)).await;
            })
            .await
            .unwrap(),
        "availability reads must not create artwork work"
    );
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other_token), StatusCode::NOT_FOUND),
    ] {
        let (status, error) = call(&app, "GET", &path, Value::Null, token).await;
        assert_eq!(status, expected);
        assert!(error.get("can_fetch").is_none());
        assert!(!error.to_string().contains(recording));
    }
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/recordings/{}/artwork/availability", Uuid::new_v4()),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(saved_state(&app, &token, playlist, recording).await, before);
    assert_eq!(provider.requests().await, requests);

    assert_eq!(
        call(
            &app,
            "DELETE",
            &installation_path,
            json!({"revision": installation["revision"]}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(get(&app, &token, &path).await, json!({"can_fetch": false}));
    assert_eq!(
        get(&app, &token, &local_path).await,
        json!({"can_fetch": false})
    );
    assert_eq!(saved_state(&app, &token, playlist, recording).await, before);
    assert_eq!(provider.requests().await, requests);
    assert_eq!(
        call(&app, "POST", "/api/v1/logout", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", &path, Value::Null, Some(&token)).await.0,
        StatusCode::UNAUTHORIZED
    );
}

async fn get(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn saved_state(app: &Router, token: &str, playlist: &str, recording: &str) -> Value {
    json!({
        "catalog": get(app, token, "/api/v1/recordings").await,
        "playlists": get(app, token, "/api/v1/playlists").await,
        "playlist": get(app, token, &format!("/api/v1/playlists/{playlist}")).await,
        "metadata": get(app, token, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "sources": get(app, token, &format!("/api/v1/recordings/{recording}/sources")).await,
        "activity": get(app, token, "/api/v1/activity").await,
        "events": get(app, token, "/api/v1/sync?cursor=0").await,
    })
}
