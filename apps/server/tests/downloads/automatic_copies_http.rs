use hito_core::test_support::{hang_guard::hang_guard, provider_audio as audio_fixture};
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod http;

use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use http::call;
use serde_json::{Value, json};
use sqlx::Connection;
use std::{future::pending, time::Duration};

const DEFAULT_BUDGET: u64 = 5 * 1024 * 1024 * 1024;

async fn token(app: &axum::Router, username: &str, password: &str) -> String {
    let (status, session) = call(
        app,
        "POST",
        "/api/v1/login",
        json!({"username": username, "password": password}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    session["token"].as_str().unwrap().to_owned()
}

async fn drain(f: &AudioLibrary) {
    while tokio::time::timeout(
        Duration::from_secs(20),
        f.hito.downloads.run_next_copy_until(pending()),
    )
    .await
    .expect("automatic copy work must finish")
    .unwrap()
    {}
}

#[tokio::test]
async fn an_announced_recording_becomes_a_saved_source_and_the_setting_reports_its_size() {
    let f = hang_guard("the provider library must open", AudioLibrary::start()).await;
    f.ready().await;
    let app = hito_server::router(f.hito.clone());
    let admin = token(&app, "admin", "indexing test password").await;
    let recording = f.source.recording_id.clone();

    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/playback/upcoming",
        json!({"client": "web-player", "recording_ids": [recording]}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    drain(&f).await;

    let (status, sources) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{recording}/sources"),
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        sources
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["provider"] == "download" && source["ready"] == true),
        "{sources}"
    );
    let (status, copies) = call(
        &app,
        "GET",
        "/api/v1/account/automatic-copies",
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        copies
            == json!({
                "keep_played_tracks": true,
                "used_bytes": f.original.len(),
                "budget_bytes": DEFAULT_BUDGET,
            }),
        "{copies}"
    );

    let (status, copies) = call(
        &app,
        "PUT",
        "/api/v1/account/automatic-copies",
        json!({"keep_played_tracks": false}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(copies["keep_played_tracks"] == false && copies["used_bytes"] == 0);
}

async fn saved(app: &axum::Router, token: &str, recording: &str) -> bool {
    let (status, sources) = call(
        app,
        "GET",
        &format!("/api/v1/recordings/{recording}/sources"),
        Value::Null,
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sources
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["provider"] == "download" && source["ready"] == true)
}

#[tokio::test]
async fn the_announced_current_recording_is_saved_and_kept_under_a_zero_budget() {
    let f = hang_guard("the provider library must open", AudioLibrary::start()).await;
    f.ready().await;
    let app = hito_server::router(f.hito.clone());
    let admin = token(&app, "admin", "indexing test password").await;
    let recording = f.source.recording_id.clone();

    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/playback/upcoming",
        json!({"client": "web-tab-1f6c", "current": recording, "recording_ids": []}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    drain(&f).await;
    assert!(saved(&app, &admin, &recording).await);

    let budget = format!("/api/v1/users/{}/automatic-copy-budget", f.actor.user().id);
    let (status, _) = call(
        &app,
        "PUT",
        &budget,
        json!({"budget_bytes": 0}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(saved(&app, &admin, &recording).await);

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/playback/upcoming",
        json!({"client": "web-tab-1f6c", "current": null, "recording_ids": []}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        "PUT",
        &budget,
        json!({"budget_bytes": 0}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!saved(&app, &admin, &recording).await);
}

#[tokio::test]
async fn a_full_play_reported_through_sync_counts_once_and_saves_a_copy() {
    let f = hang_guard("the provider library must open", AudioLibrary::start()).await;
    f.ready().await;
    let app = hito_server::router(f.hito.clone());
    let admin = token(&app, "admin", "indexing test password").await;
    let recording = f.source.recording_id.clone();
    let mut database = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE recordings SET duration_ms=200000 WHERE id=?")
        .bind(&recording)
        .execute(&mut database)
        .await
        .unwrap();

    for position_ms in [20_000, 120_000, 190_000, 200_000] {
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/sync",
            json!({"device_id": "web-tab-1f6c", "operations": [{
                "id": uuid::Uuid::new_v4(),
                "action": {"kind": "progress_set", "recording_id": recording, "position_ms": position_ms, "speed": 1.0}
            }]}),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let plays: i64 = sqlx::query_scalar(
        "SELECT play_count FROM listening_history WHERE user_id=? AND recording_id=?",
    )
    .bind(&f.actor.user().id)
    .bind(&recording)
    .fetch_one(&mut database)
    .await
    .unwrap();
    assert_eq!(plays, 1);
    drain(&f).await;
    assert!(saved(&app, &admin, &recording).await);
}

#[tokio::test]
async fn announcements_are_validated_authenticated_and_paced() {
    let f = hang_guard("the provider library must open", AudioLibrary::start()).await;
    let app = hito_server::router(f.hito.clone());
    let admin = token(&app, "admin", "indexing test password").await;
    let recording = f.source.recording_id.clone();
    let path = "/api/v1/playback/upcoming";

    assert_eq!(
        call(&app, "POST", path, json!({"recording_ids": []}), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    for (body, expected) in [
        (
            json!({"recording_ids": [recording, recording, recording]}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"recording_ids": ["not-a-recording"]}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"client": "spaces are not allowed", "recording_ids": []}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"recording_ids": [], "unexpected": true}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"current": "not-a-recording", "recording_ids": []}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        assert_eq!(
            call(&app, "POST", path, body, Some(&admin)).await.0,
            expected
        );
    }
    assert_eq!(
        f.hito
            .downloads
            .automatic_copies(&f.actor)
            .await
            .unwrap()
            .used_bytes,
        0
    );

    let mut statuses = Vec::new();
    for _ in 0..31 {
        statuses.push(
            call(
                &app,
                "POST",
                path,
                json!({"recording_ids": [recording]}),
                Some(&admin),
            )
            .await
            .0,
        );
    }
    assert!(
        statuses[..30]
            .iter()
            .all(|status| *status == StatusCode::NO_CONTENT)
    );
    assert_eq!(statuses[30], StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn only_an_administrator_reads_or_sets_an_accounts_copy_budget() {
    let f = hang_guard("the provider library must open", AudioLibrary::start()).await;
    let app = hito_server::router(f.hito.clone());
    let admin = token(&app, "admin", "indexing test password").await;
    let (status, member) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "member", "password": "automatic copy member password"}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let member_id = member["id"].as_str().unwrap().to_owned();
    let member_token = token(&app, "member", "automatic copy member password").await;
    let path = format!("/api/v1/users/{member_id}/automatic-copy-budget");

    let (status, budget) = call(&app, "GET", &path, Value::Null, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(budget == json!({"budget_bytes": DEFAULT_BUDGET}));
    let (status, budget) = call(
        &app,
        "PUT",
        &path,
        json!({"budget_bytes": 1_073_741_824_u64}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(budget == json!({"budget_bytes": 1_073_741_824_u64}));
    let (status, copies) = call(
        &app,
        "GET",
        "/api/v1/account/automatic-copies",
        Value::Null,
        Some(&member_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(copies["budget_bytes"] == 1_073_741_824_u64);

    for (method, body) in [("GET", Value::Null), ("PUT", json!({"budget_bytes": 0}))] {
        assert_eq!(
            call(&app, method, &path, body, Some(&member_token)).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            json!({"budget_bytes": 2_199_023_255_552_u64}),
            Some(&admin),
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/users/00000000-0000-0000-0000-000000000000/automatic-copy-budget",
            Value::Null,
            Some(&admin),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
