use super::support::{admin, call, services, write_wav};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn administrator_can_browse_connect_scan_and_remove_a_music_folder() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("PRIVATE_MUSIC_PATH");
    std::fs::create_dir_all(music.join("Albums")).unwrap();
    write_wav(&music.join("Albums/Fixture.wav"));
    let core = services(dir.path(), &music).await;
    let app = hito_server::router(core.clone());
    let token = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    let (status, shares) = call(
        &app,
        "GET",
        "/api/v1/storage/directories",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shares[0]["available"], true);
    assert!(!shares.to_string().contains("PRIVATE_MUSIC_PATH"));
    let (status, page) = call(
        &app,
        "POST",
        "/api/v1/storage/browse",
        json!({"directory":{"directory_id":"legacy-music","components":[]}}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["directories"][0]["name"], "Albums");
    let url = format!("/api/v1/libraries/{library}/folders");
    let (status, folder) = call(
        &app,
        "POST",
        &url,
        json!({"directory":page["directories"][0]["directory"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(core.storage.run_next_job().await.unwrap());
    let (status, folders) = call(&app, "GET", &url, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(folders[0]["latest_job"]["state"], "complete");
    assert!(!folders.to_string().contains("PRIVATE_MUSIC_PATH"));
    let (_, tracks) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    assert_eq!(tracks["items"][0]["available"], true);
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/folders/{}", folder["id"].as_str().unwrap()),
        json!({"revision":folder["revision"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, tracks) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    assert_eq!(tracks["total"], 1);
    assert_eq!(tracks["items"][0]["available"], false);
    assert!(music.join("Albums/Fixture.wav").exists());
}

#[tokio::test]
async fn storage_administration_requires_an_administrator_and_rejects_host_paths() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("PRIVATE_MUSIC_PATH");
    std::fs::create_dir(&music).unwrap();
    let core = services(dir.path(), &music).await;
    let app = hito_server::router(core);
    let admin_token = admin(&app).await;
    let (_, libraries) = call(
        &app,
        "GET",
        "/api/v1/libraries",
        Value::Null,
        Some(&admin_token),
    )
    .await;
    let library = libraries[0]["id"].as_str().unwrap();
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"listener","password":"listener folder password"}),
        Some(&admin_token),
    )
    .await;
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"listener","password":"listener folder password"}),
        None,
    )
    .await;
    let listener = session["token"].as_str().unwrap();
    for (method, url, body) in [
        ("GET", "/api/v1/storage/directories".into(), Value::Null),
        (
            "POST",
            "/api/v1/storage/browse".into(),
            json!({"directory":{"directory_id":"legacy-music","components":[]}}),
        ),
        (
            "GET",
            format!("/api/v1/libraries/{library}/folders"),
            Value::Null,
        ),
        (
            "POST",
            format!("/api/v1/libraries/{library}/folders"),
            json!({"directory":{"directory_id":"legacy-music","components":[]}}),
        ),
        (
            "DELETE",
            "/api/v1/folders/unknown".into(),
            json!({"revision":1}),
        ),
        ("POST", "/api/v1/folders/unknown/scan".into(), Value::Null),
        (
            "DELETE",
            format!("/api/v1/libraries/{library}/missing-recordings"),
            Value::Null,
        ),
    ] {
        assert_eq!(
            call(&app, method, &url, body.clone(), None).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, method, &url, body, Some(listener)).await.0,
            StatusCode::FORBIDDEN
        );
    }
    for body in [
        json!({"directory":{"directory_id":"legacy-music","components":[".."]}}),
        json!({"directory":{"directory_id":"legacy-music","components":[],"root":"PRIVATE_HOST_PATH"}}),
    ] {
        let (status, error) = call(
            &app,
            "POST",
            "/api/v1/storage/browse",
            body,
            Some(&admin_token),
        )
        .await;
        assert!(status.is_client_error());
        assert!(!error.to_string().contains("PRIVATE_"));
    }
}

#[tokio::test]
async fn administrator_removes_recordings_whose_files_are_gone_now() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("PRIVATE_MUSIC_PATH");
    std::fs::create_dir_all(&music).unwrap();
    write_wav(&music.join("Gone.wav"));
    let core = services(dir.path(), &music).await;
    let app = hito_server::router(core.clone());
    let token = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    let folders_url = format!("/api/v1/libraries/{library}/folders");
    let (status, folder) = call(
        &app,
        "POST",
        &folders_url,
        json!({"directory":{"directory_id":"legacy-music","components":[]}}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(core.storage.run_next_job().await.unwrap());
    std::fs::remove_file(music.join("Gone.wav")).unwrap();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/folders/{}/scan", folder["id"].as_str().unwrap()),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(core.storage.run_next_job().await.unwrap());
    let (_, tracks) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    assert_eq!(tracks["total"], 1);
    let url = format!("/api/v1/libraries/{library}/missing-recordings");
    let (status, outcome) = call(&app, "DELETE", &url, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        outcome,
        json!({"removed":1,"kept_referenced":0,"more":false})
    );
    let (_, tracks) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    assert_eq!(tracks["total"], 0);
    let (status, error) = call(
        &app,
        "DELETE",
        "/api/v1/libraries/unknown/missing-recordings",
        Value::Null,
        Some(&token),
    )
    .await;
    assert!(status.is_client_error());
    assert!(!error.to_string().contains("PRIVATE_"));
}
