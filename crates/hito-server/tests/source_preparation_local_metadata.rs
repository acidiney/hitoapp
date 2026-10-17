#[allow(dead_code)]
#[path = "api/support.rs"]
mod support;

use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use support::{call, indexed_app};

#[tokio::test]
async fn shared_local_sources_keep_safe_labels_when_the_music_folder_is_disconnected() {
    let (directory, app, owner, recording) = indexed_app().await;
    let original = directory.path().join("music/Fixture.wav");
    let original_audio = std::fs::read(&original).unwrap();
    let source_path = format!("/api/v1/recordings/{recording}/sources");
    let before = local_source(&app, &owner, &source_path, true).await;
    let source_id = before["id"].as_str().unwrap();
    let (status, libraries) =
        call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    let library = libraries[0]["id"].as_str().unwrap();
    let folder_path = format!("/api/v1/libraries/{library}/folders");
    let (status, folders) = call(&app, "GET", &folder_path, Value::Null, Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(folders.as_array().unwrap().len(), 1);

    let account = json!({
        "username": "listener", "password": "owned shared local metadata password"
    });
    let (status, listener) =
        call(&app, "POST", "/api/v1/users", account.clone(), Some(&owner)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, session) = call(&app, "POST", "/api/v1/login", account, None).await;
    assert_eq!(status, StatusCode::OK);
    let listener_token = session["token"].as_str().unwrap();
    let access_path = format!(
        "/api/v1/libraries/{library}/members/{}",
        listener["id"].as_str().unwrap()
    );
    let (status, _) = call(&app, "PUT", &access_path, Value::Null, Some(&owner)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let shared = local_source(&app, listener_token, &source_path, true).await;
    assert!(shared["id"].as_str() == Some(source_id));

    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/folders/{}", folders[0]["id"].as_str().unwrap()),
        json!({"revision": folders[0]["revision"]}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for token in [owner.as_str(), listener_token] {
        let disconnected = local_source(&app, token, &source_path, false).await;
        assert!(disconnected["id"].as_str() == Some(source_id));
        assert!(disconnected["recording_id"].as_str() == Some(recording.as_str()));
        let (status, _) = call(
            &app,
            "POST",
            &format!("{source_path}/{source_id}/preparation"),
            json!({"request_id": "a2aa94f0-9255-4e66-a7ce-6db8c64e76d0"}),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let retained_audio = std::fs::read(&original).unwrap();
    assert!(
        retained_audio == original_audio,
        "disconnect must preserve the original audio"
    );
}

async fn local_source(app: &Router, token: &str, path: &str, ready: bool) -> Value {
    let (status, sources) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    let encoded = sources.to_string();
    assert!(
        !encoded.contains('/')
            && !encoded.contains('\\')
            && !encoded.contains("Fixture")
            && !encoded.contains("provider_id"),
        "local source metadata must omit file paths, file names and provider locators"
    );
    let sources = sources.as_array().unwrap();
    assert_eq!(sources.len(), 1);
    let source = &sources[0];
    assert!(source["label"].as_str() == Some("Local audio"));
    assert!(source["provider"].as_str() == Some("local"));
    assert!(source["ready"].as_bool() == Some(ready));
    assert!(source["can_prepare"].as_bool() == Some(false));
    source.clone()
}
