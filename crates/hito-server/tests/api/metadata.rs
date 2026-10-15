use super::support::{call, indexed_app};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn metadata_endpoint_explains_personal_and_inherited_titles_without_exposing_paths() {
    let (dir, app, token, recording) = indexed_app().await;
    let path = format!("/api/v1/recordings/{recording}/metadata");
    assert_eq!(
        call(&app, "GET", &path, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, detail) = call(&app, "GET", &path, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["title"]["value"], "Fixture");
    assert_eq!(detail["selected_observation"]["origin"], "file_inspection");
    assert!(detail["selected_observation"]["tags"]["title"].is_null());
    assert_eq!(detail["observations"]["items"].as_array().unwrap().len(), 1);
    assert!(!detail.to_string().contains(dir.path().to_str().unwrap()));
    assert!(!detail.to_string().contains("file_checksum"));
    let operation = Uuid::new_v4();
    let batch = json!({"device_id":"metadata-client","operations":[{"id":operation,"action":{"kind":"recording_title_set","recording_id":recording,"title":"My private title"}}]});
    for _ in 0..2 {
        let (status, receipt) =
            call(&app, "POST", "/api/v1/sync", batch.clone(), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(receipt["acknowledged"][0], operation.to_string());
    }
    let detail = call(&app, "GET", &path, Value::Null, Some(&token)).await.1;
    assert_eq!(detail["title"]["value"], "My private title");
    assert_eq!(detail["title"]["personal"], "My private title");
    assert_eq!(detail["title"]["inherited"], "Fixture");
    assert_eq!(detail["title"]["operation_id"], operation.to_string());
    let (_, bob) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"bob","password":"metadata bob password"}),
        Some(&token),
    )
    .await;
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"bob","password":"metadata bob password"}),
        None,
    )
    .await;
    let bob_token = session["token"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", &path, Value::Null, Some(bob_token))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let libs = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token))
        .await
        .1;
    call(
        &app,
        "PUT",
        &format!(
            "/api/v1/libraries/{}/members/{}",
            libs[0]["id"].as_str().unwrap(),
            bob["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    let detail = call(&app, "GET", &path, Value::Null, Some(bob_token))
        .await
        .1;
    assert_eq!(detail["title"]["value"], "Fixture");
    assert!(detail["title"]["personal"].is_null());
    assert!(!detail.to_string().contains("My private title"));
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("{path}?before=-1"),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
