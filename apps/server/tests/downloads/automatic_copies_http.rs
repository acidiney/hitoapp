use hito_core::test_support::{hang_guard::hang_guard, provider_audio as audio_fixture};
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod http;

use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use http::call;
use serde_json::{Value, json};
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
