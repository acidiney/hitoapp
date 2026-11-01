use super::{
    playlist_fixture,
    provider_fixture::ProviderServer,
    support::{admin, app, call},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

async fn accept(
    app: &Router,
    token: &str,
    provider: &ProviderServer,
    path: &str,
    revision: i64,
    name: &str,
    capabilities: &[&str],
) -> Value {
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.http-playlist-fixture", "name": name,
                "protocol_version": 1, "capabilities": capabilities
            }),
        )
        .await;
    let (status, review) = call(
        app,
        "POST",
        &format!("{path}/refresh"),
        json!({"revision": revision}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, accepted) = call(
        app,
        "POST",
        &format!("{path}/refresh"),
        json!({"revision": revision, "accept_manifest_digest": review["digest"]}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    accepted["installation"].clone()
}

async fn followed(
    app: &Router,
    token: &str,
) -> (playlist_fixture::PlaylistProvider, String, String) {
    let provider = playlist_fixture::connect(app, token).await;
    let path = format!("/api/v1/plugins/{}", provider.installation_id);
    let (status, receipt) = call(
        app,
        "POST",
        &format!("{path}/playlists/follow"),
        json!({
            "request_id": Uuid::new_v4(), "installation_revision": provider.revision,
            "playlist_reference": "https://provider.invalid/owned-auto-resume", "interval": "daily"
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let playlist = receipt["playlist_id"].as_str().unwrap();
    (
        provider,
        path,
        format!("/api/v1/playlists/{playlist}/follow"),
    )
}

async fn status(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

#[tokio::test]
async fn a_renamed_connection_keeps_the_follow_ready_over_http() {
    let (_directory, app) = app().await;
    let owner = admin(&app).await;
    let (provider, path, follow) = followed(&app, &owner).await;
    let before = status(&app, &owner, &follow).await;
    assert_eq!(before["status"], "ready");
    assert!(before["review"].is_null());
    accept(
        &app,
        &owner,
        &provider.server,
        &path,
        provider.revision,
        "Rebuilt HTTP fixture",
        &["playlists"],
    )
    .await;
    let after = status(&app, &owner, &follow).await;
    assert_ne!(after["status"], "paused");
    assert_eq!(
        after["generation"],
        before["generation"].as_i64().unwrap() + 1
    );
    assert!(after["review"].is_null());
}

#[tokio::test]
async fn a_gained_ability_pauses_the_follow_with_a_review_over_http() {
    let (_directory, app) = app().await;
    let owner = admin(&app).await;
    let (provider, path, follow) = followed(&app, &owner).await;
    let installation = accept(
        &app,
        &owner,
        &provider.server,
        &path,
        provider.revision,
        "Wider HTTP fixture",
        &["playlists", "search"],
    )
    .await;
    let paused = status(&app, &owner, &follow).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["review"]["connection_name"], "Wider HTTP fixture");
    assert_eq!(paused["review"]["resumable"], true);
    assert_eq!(
        paused["review"]["installation_revision"],
        installation["revision"]
    );
    assert_eq!(paused["review"]["changes"][0]["kind"], "capabilities");
    assert_eq!(
        paused["review"]["changes"][0]["before"],
        json!(["playlists"])
    );
}
