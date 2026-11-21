use super::{
    playlist_fixture,
    support::{admin, app, call},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

struct Followed {
    provider: playlist_fixture::PlaylistProvider,
    service: Value,
    playlist: String,
    follow: String,
}

async fn followed(app: &Router, token: &str) -> Followed {
    let provider = playlist_fixture::connect(app, token).await;
    let (status, receipt) = call(
        app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/follow",
            provider.installation_id
        ),
        json!({
            "request_id": Uuid::new_v4(), "installation_revision": provider.revision,
            "playlist_reference": "https://provider.invalid/owned-reconnect", "interval": "daily"
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, settings) = call(app, "GET", "/api/v1/plugins", Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    let service = settings["services"][0].clone();
    let playlist = receipt["playlist_id"].as_str().unwrap().to_owned();
    Followed {
        provider,
        service,
        follow: format!("/api/v1/playlists/{playlist}/follow"),
        playlist,
    }
}

async fn remove(app: &Router, token: &str, installation: &str, revision: i64) {
    let (status, _) = call(
        app,
        "DELETE",
        &format!("/api/v1/plugins/{installation}"),
        json!({"revision": revision}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

async fn connect(app: &Router, token: &str, scenario: &Followed, capabilities: &[&str]) -> Value {
    scenario
        .provider
        .server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.http-playlist-fixture", "name": "HTTP playlist fixture",
                "protocol_version": 1, "capabilities": capabilities
            }),
        )
        .await;
    let (status, installation) = call(
        app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": scenario.service["id"], "service_revision": scenario.service["revision"]}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    installation
}

async fn status(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn refresh(app: &Router, token: &str, scenario: &Followed) -> StatusCode {
    call(
        app,
        "POST",
        &format!("{}/refresh", scenario.follow),
        json!({"request_id": Uuid::new_v4()}),
        Some(token),
    )
    .await
    .0
}

async fn resume(app: &Router, token: &str, scenario: &Followed, paused: &Value) -> StatusCode {
    call(
        app,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": "http-follow-reconnect",
            "operations": [{"id": Uuid::new_v4(), "action": {
                "kind": "playlist_follow_resume",
                "playlist_id": scenario.playlist,
                "expected_generation": paused["generation"],
                "installation_revision": paused["review"]["installation_revision"],
                "service_revision": paused["review"]["service_revision"]
            }}]
        }),
        Some(token),
    )
    .await
    .0
}

async fn member(app: &Router, owner: &str) -> String {
    let credentials = json!({"username": "member", "password": "member reconnect password"});
    assert_eq!(
        call(
            app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(owner)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, session) = call(app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    session["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_removed_connection_names_the_plugin_to_connect_again_and_a_reconnect_resumes_it() {
    let (_directory, app) = app().await;
    let owner = admin(&app).await;
    let scenario = followed(&app, &owner).await;
    remove(
        &app,
        &owner,
        &scenario.provider.installation_id,
        scenario.provider.revision,
    )
    .await;
    let removed = status(&app, &owner, &scenario.follow).await;
    assert_eq!(removed["status"], "paused");
    assert_eq!(removed["review"]["resumable"], false);
    assert_eq!(removed["review"]["changes"][0]["kind"], "removed");
    assert_eq!(
        removed["review"]["connection_name"],
        "HTTP playlist fixture"
    );
    assert_eq!(
        removed["review"]["reconnect_service_id"],
        scenario.service["id"]
    );
    assert_eq!(refresh(&app, &owner, &scenario).await, StatusCode::CONFLICT);

    connect(&app, &owner, &scenario, &["playlists"]).await;
    let carried = status(&app, &owner, &scenario.follow).await;
    assert_ne!(carried["status"], "paused");
    assert!(carried["review"].is_null());
    assert_eq!(refresh(&app, &owner, &scenario).await, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn a_reconnect_with_changed_abilities_shows_resume_over_http() {
    let (_directory, app) = app().await;
    let owner = admin(&app).await;
    let scenario = followed(&app, &owner).await;
    remove(
        &app,
        &owner,
        &scenario.provider.installation_id,
        scenario.provider.revision,
    )
    .await;
    let replacement = connect(&app, &owner, &scenario, &["playlists", "lyrics"]).await;
    let paused = status(&app, &owner, &scenario.follow).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["review"]["resumable"], true);
    assert_eq!(
        paused["review"]["installation_revision"],
        replacement["revision"]
    );
    assert_eq!(paused["review"]["changes"][0]["kind"], "capabilities");
    assert!(paused["review"]["reconnect_service_id"].is_null());
    assert_eq!(
        resume(&app, &owner, &scenario, &paused).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, &owner, &scenario.follow).await["status"],
        "ready"
    );
    assert_eq!(refresh(&app, &owner, &scenario).await, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn another_accounts_connection_never_resumes_the_owners_follow_over_http() {
    let (_directory, app) = app().await;
    let owner = admin(&app).await;
    let scenario = followed(&app, &owner).await;
    remove(
        &app,
        &owner,
        &scenario.provider.installation_id,
        scenario.provider.revision,
    )
    .await;
    let other = member(&app, &owner).await;
    connect(&app, &other, &scenario, &["playlists"]).await;
    let removed = status(&app, &owner, &scenario.follow).await;
    assert_eq!(removed["status"], "paused");
    assert_eq!(removed["review"]["changes"][0]["kind"], "removed");
    assert_eq!(refresh(&app, &owner, &scenario).await, StatusCode::CONFLICT);
    assert_eq!(
        call(&app, "GET", &scenario.follow, Value::Null, Some(&other))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_follow_stranded_on_an_old_connection_recovers_over_http() {
    let (directory, app) = app().await;
    let owner = admin(&app).await;
    let scenario = followed(&app, &owner).await;
    remove(
        &app,
        &owner,
        &scenario.provider.installation_id,
        scenario.provider.revision,
    )
    .await;
    connect(&app, &owner, &scenario, &["playlists"]).await;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE playlist_follows SET installation_id = ?,installation_revision = ? WHERE playlist_id = ?")
        .bind(&scenario.provider.installation_id)
        .bind(scenario.provider.revision)
        .bind(&scenario.playlist)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let recovered = status(&app, &owner, &scenario.follow).await;
    assert_ne!(recovered["status"], "paused");
    assert!(recovered["review"].is_null());
    assert_eq!(refresh(&app, &owner, &scenario).await, StatusCode::ACCEPTED);
}
