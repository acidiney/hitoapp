use super::{
    playlist_fixture,
    provider_fixture::ProviderServer,
    support::{admin, app, call},
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

const DEVICE: &str = "http-follow-resume-access";
const ALREADY_APPROVED: &str = "This playlist connection is already approved for updates";
const GENERATION_CHANGED: &str = "This playlist changed. Review its connection again.";
const REVIEW_CHANGED: &str = "The reviewed playlist connection changed. Review it again.";

#[tokio::test]
async fn resume_requires_the_owned_reviewed_connection_and_cannot_resurrect_a_deleted_follow() {
    let (_directory, app) = app().await;
    let owner = admin(&app).await;
    let credentials = json!({"username": "other", "password": "owned resume access password"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(&owner)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other = session["token"].as_str().unwrap();
    let tokens = [owner.as_str(), other];
    let provider = playlist_fixture::connect(&app, &owner).await;
    let connection_path = format!("/api/v1/plugins/{}", provider.installation_id);
    let (status, receipt) = call(&app, "POST", &format!("{connection_path}/playlists/follow"), json!({
        "request_id": Uuid::new_v4(), "installation_revision": provider.revision,
        "playlist_reference": "https://provider.invalid/owned-resume-access", "interval": "daily"
    }), Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    let playlist = receipt["playlist_id"].as_str().unwrap();
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let status_path = format!("{detail_path}/follow");
    let detail = get(&app, &owner, &detail_path).await;
    let recording = detail["entries"][0]["recording_id"].as_str().unwrap();
    let preferences = json!({"device_id": DEVICE, "operations": [
        {"id": Uuid::new_v4(), "action": {"kind": "recording_title_set", "recording_id": recording, "title": "My private resume title"}},
        {"id": Uuid::new_v4(), "action": {"kind": "like_set", "recording_id": recording, "liked": true}},
        {"id": Uuid::new_v4(), "action": {"kind": "progress_set", "recording_id": recording, "position_ms": 4200, "speed": 1.25}}
    ]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", preferences, Some(&owner))
            .await
            .0,
        StatusCode::OK
    );
    let saved = get(&app, &owner, &detail_path).await;
    let initial = state(&app, &owner, playlist).await;
    let other_initial = state(&app, other, playlist).await;
    assert_eq!(initial["catalog"]["total"], 1);
    assert_eq!(initial["catalog"]["items"][0]["liked"], true);
    assert_eq!(initial["activity"]["history"][0]["position_ms"], 4200);
    assert_eq!(other_initial["catalog"]["total"], 0);
    assert_eq!(other_initial["playlists"], json!([]));
    assert_eq!(other_initial["detail_status"], 404);
    assert_eq!(other_initial["follow_status"], 404);
    assert_ne!(initial["identity"]["id"], other_initial["identity"]["id"]);
    let service = initial["plugins"]["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == saved["origin"]["service_id"])
        .unwrap();
    let service_id = service["id"].as_str().unwrap();
    let service_revision = service["revision"].as_i64().unwrap();
    let generation = initial["follow"]["generation"].as_i64().unwrap();
    assert_eq!(generation, 1);
    assert_eq!(initial["follow"]["status"], "ready");
    let requests = provider_requests(&provider.server).await;
    assert_eq!(requests, [2, 1, 0, 0, 0, 0]);
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(&owner),
        resume(
            playlist,
            Uuid::new_v4(),
            generation,
            provider.revision,
            service_revision,
        ),
        (StatusCode::CONFLICT, ALREADY_APPROVED),
    )
    .await;

    let approved = approve(
        &app,
        &owner,
        &provider.server,
        &connection_path,
        provider.revision,
        &["playlists", "search"],
    )
    .await;
    let approved_revision = approved["revision"].as_i64().unwrap();
    let paused = get(&app, &owner, &status_path).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["generation"], generation);
    assert_eq!(get(&app, &owner, &detail_path).await, saved);
    let accepted_id = Uuid::new_v4();
    let accepted_batch = resume(
        playlist,
        accepted_id,
        generation,
        approved_revision,
        service_revision,
    );
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(other),
        accepted_batch.clone(),
        (StatusCode::NOT_FOUND, "Resource not found"),
    )
    .await;
    for body in [
        resume(
            playlist,
            Uuid::new_v4(),
            generation + 1,
            approved_revision,
            service_revision,
        ),
        resume(
            playlist,
            Uuid::new_v4(),
            generation,
            provider.revision,
            service_revision,
        ),
        resume(
            playlist,
            Uuid::new_v4(),
            generation,
            approved_revision,
            service_revision + 1,
        ),
    ] {
        let message = if body["operations"][0]["action"]["expected_generation"] == generation + 1 {
            GENERATION_CHANGED
        } else {
            REVIEW_CHANGED
        };
        reject(
            &app,
            &provider.server,
            tokens,
            playlist,
            Some(&owner),
            body,
            (StatusCode::CONFLICT, message),
        )
        .await;
    }
    let before_resume = state(&app, &owner, playlist).await;
    acknowledge(&app, &owner, accepted_batch.clone()).await;
    let resumed = state(&app, &owner, playlist).await;
    assert_eq!(resumed["follow"]["generation"], generation + 1);
    assert_eq!(resumed["follow"]["status"], "ready");
    assert!(resumed["follow"]["error"].is_null());
    for field in [
        "identity",
        "plugins",
        "catalog",
        "activity",
        "detail",
        "playlists",
    ] {
        assert_eq!(resumed[field], before_resume[field]);
    }
    appended(&before_resume, &resumed, &accepted_batch);
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(other),
        accepted_batch.clone(),
        (StatusCode::NOT_FOUND, "Resource not found"),
    )
    .await;
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(&owner),
        resume(
            playlist,
            Uuid::new_v4(),
            generation,
            approved_revision,
            service_revision,
        ),
        (StatusCode::CONFLICT, GENERATION_CHANGED),
    )
    .await;
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(&owner),
        resume(
            playlist,
            Uuid::new_v4(),
            generation + 1,
            approved_revision,
            service_revision,
        ),
        (StatusCode::CONFLICT, ALREADY_APPROVED),
    )
    .await;

    let removed_capability = approve(
        &app,
        &owner,
        &provider.server,
        &connection_path,
        approved_revision,
        &["search"],
    )
    .await;
    let removed_revision = removed_capability["revision"].as_i64().unwrap();
    assert_eq!(get(&app, &owner, &status_path).await["status"], "paused");
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(&owner),
        resume(
            playlist,
            Uuid::new_v4(),
            generation + 1,
            removed_revision,
            service_revision,
        ),
        (StatusCode::CONFLICT, REVIEW_CHANGED),
    )
    .await;
    let restored = approve(
        &app,
        &owner,
        &provider.server,
        &connection_path,
        removed_revision,
        &["playlists", "search"],
    )
    .await;
    let restored_revision = restored["revision"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &connection_path,
            json!({"revision": restored_revision}),
            Some(&owner)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(&owner),
        resume(
            playlist,
            Uuid::new_v4(),
            generation + 1,
            restored_revision,
            service_revision,
        ),
        (StatusCode::CONFLICT, REVIEW_CHANGED),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/plugins/services/{service_id}"),
            json!({"revision": service_revision}),
            Some(&owner)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(&owner),
        resume(
            playlist,
            Uuid::new_v4(),
            generation + 1,
            restored_revision,
            service_revision,
        ),
        (StatusCode::CONFLICT, REVIEW_CHANGED),
    )
    .await;
    assert_eq!(get(&app, &owner, &detail_path).await, saved);

    let deletion = batch(
        Uuid::new_v4(),
        json!({"kind": "playlist_delete", "playlist_id": playlist}),
    );
    let before_delete = state(&app, &owner, playlist).await;
    acknowledge(&app, &owner, deletion.clone()).await;
    let deleted = state(&app, &owner, playlist).await;
    assert_eq!(deleted["playlists"], json!([]));
    assert_eq!(deleted["detail_status"], 404);
    assert_eq!(deleted["follow_status"], 404);
    for field in ["identity", "plugins", "catalog", "activity"] {
        assert_eq!(deleted[field], before_delete[field]);
    }
    appended(&before_delete, &deleted, &deletion);
    let late = resume(
        playlist,
        Uuid::new_v4(),
        generation,
        provider.revision,
        service_revision,
    );
    acknowledge(&app, &owner, late.clone()).await;
    let after_late = state(&app, &owner, playlist).await;
    for field in [
        "identity",
        "plugins",
        "catalog",
        "activity",
        "detail_status",
        "detail",
        "follow_status",
        "follow",
        "playlists",
    ] {
        assert_eq!(after_late[field], deleted[field]);
    }
    appended(&deleted, &after_late, &late);
    acknowledge(&app, &owner, accepted_batch.clone()).await;
    assert_eq!(state(&app, &owner, playlist).await, after_late);
    let mut changed = accepted_batch;
    changed["operations"][0]["action"]["expected_generation"] = json!(generation + 1);
    reject(
        &app,
        &provider.server,
        tokens,
        playlist,
        Some(&owner),
        changed,
        (
            StatusCode::CONFLICT,
            "An operation ID was reused with different contents",
        ),
    )
    .await;
    assert_eq!(
        get(&app, other, "/api/v1/me").await,
        other_initial["identity"]
    );
    assert_eq!(get(&app, &owner, "/api/v1/me").await, initial["identity"]);
    assert_eq!(get(&app, other, "/api/v1/playlists").await, json!([]));
    assert_eq!(
        get(&app, other, "/api/v1/sync?cursor=0").await,
        other_initial["feed"]
    );
    assert_eq!(provider_requests(&provider.server).await, requests);
}

fn resume(
    playlist: &str,
    operation: Uuid,
    generation: i64,
    installation: i64,
    service: i64,
) -> Value {
    batch(
        operation,
        json!({
            "kind": "playlist_follow_resume", "playlist_id": playlist,
            "expected_generation": generation, "installation_revision": installation,
            "service_revision": service
        }),
    )
}

fn batch(operation: Uuid, action: Value) -> Value {
    json!({"device_id": DEVICE, "operations": [{"id": operation, "action": action}]})
}

async fn acknowledge(app: &Router, token: &str, body: Value) {
    let (status, response) = call(app, "POST", "/api/v1/sync", body.clone(), Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response,
        json!({"acknowledged": [body["operations"][0]["id"]]})
    );
}

async fn reject(
    app: &Router,
    provider: &ProviderServer,
    tokens: [&str; 2],
    playlist: &str,
    token: Option<&str>,
    body: Value,
    expected: (StatusCode, &str),
) {
    let owner = state(app, tokens[0], playlist).await;
    let other = state(app, tokens[1], playlist).await;
    let observed = provider_requests(provider).await;
    let (status, response) = call(app, "POST", "/api/v1/sync", body, token).await;
    assert_eq!(status, expected.0);
    assert_eq!(response, json!({"error": expected.1}));
    assert_eq!(state(app, tokens[0], playlist).await, owner);
    assert_eq!(state(app, tokens[1], playlist).await, other);
    assert_eq!(provider_requests(provider).await, observed);
}

async fn approve(
    app: &Router,
    token: &str,
    provider: &ProviderServer,
    path: &str,
    revision: i64,
    capabilities: &[&str],
) -> Value {
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.http-playlist-fixture", "name": "Reviewed HTTP playlist fixture",
                "protocol_version": 1, "capabilities": capabilities
            }),
        )
        .await;
    let settings = get(app, token, "/api/v1/plugins").await;
    let (status, review) = call(
        app,
        "POST",
        &format!("{path}/refresh"),
        json!({"revision": revision}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["kind"], "review_required");
    assert!(review["digest"].is_string());
    assert_eq!(get(app, token, "/api/v1/plugins").await, settings);
    let (status, accepted) = call(
        app,
        "POST",
        &format!("{path}/refresh"),
        json!({
            "revision": revision, "accept_manifest_digest": review["digest"]
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accepted["kind"], "updated");
    assert_eq!(accepted["installation"]["revision"], revision + 1);
    assert_eq!(accepted["installation"]["manifest"], review["manifest"]);
    accepted["installation"].clone()
}

fn appended(before: &Value, after: &Value, body: &Value) {
    let previous = before["feed"]["operations"].as_array().unwrap();
    let events = after["feed"]["operations"].as_array().unwrap();
    assert_eq!(events.len(), previous.len() + 1);
    assert_eq!(&events[..previous.len()], previous.as_slice());
    assert_eq!(events.last().unwrap()["operation"], body["operations"][0]);
    assert_eq!(events.last().unwrap()["device_id"], DEVICE);
    assert!(events.last().unwrap().get("publication").is_none());
    assert_eq!(after["feed"]["has_more"], false);
}

async fn get(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn state(app: &Router, token: &str, playlist: &str) -> Value {
    let path = format!("/api/v1/playlists/{playlist}");
    let (detail_status, detail) = call(app, "GET", &path, Value::Null, Some(token)).await;
    let (follow_status, follow) = call(
        app,
        "GET",
        &format!("{path}/follow"),
        Value::Null,
        Some(token),
    )
    .await;
    json!({
        "identity": get(app, token, "/api/v1/me").await,
        "plugins": get(app, token, "/api/v1/plugins").await,
        "playlists": get(app, token, "/api/v1/playlists").await,
        "catalog": get(app, token, "/api/v1/recordings").await,
        "activity": get(app, token, "/api/v1/activity").await,
        "feed": get(app, token, "/api/v1/sync?cursor=0").await,
        "detail_status": detail_status.as_u16(), "detail": detail,
        "follow_status": follow_status.as_u16(), "follow": follow,
    })
}

async fn provider_requests(provider: &ProviderServer) -> [usize; 6] {
    [
        provider.requests("v1/playlists/detail").await.len(),
        provider.requests("v1/playlists/items").await.len(),
        provider.requests("v1/search").await.len(),
        provider.requests("v1/recordings/detail").await.len(),
        provider.requests("v1/sources").await.len(),
        provider.requests("v1/audio").await.len(),
    ]
}
