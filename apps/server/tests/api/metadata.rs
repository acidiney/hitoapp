use super::support::{call, indexed_app};
use axum::http::StatusCode;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
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
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/recordings/{recording}"))
                .header("authorization", format!("Bearer {token}"))
                .header("x-inertia", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["component"], "RecordingDetailsPage");
    assert_eq!(
        page["props"]["detail"]["title"]["value"],
        "My private title"
    );
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

#[tokio::test]
async fn album_details_keep_personal_fields_private_and_preserve_track_performers() {
    let (_dir, app, token, recording) = indexed_app().await;
    let (_, releases) = call(&app, "GET", "/api/v1/releases", Value::Null, Some(&token)).await;
    let album = &releases["items"][0];
    let release = album["id"].as_str().unwrap();
    let path = format!("/api/v1/releases/{release}/metadata");
    assert_eq!(
        call(&app, "GET", &path, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, original) = call(&app, "GET", &path, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let batch = json!({"device_id":"metadata-client","operations":[
        {"id":Uuid::new_v4(),"action":{"kind":"release_title_set","release_id":release,"title":"My album"}},
        {"id":Uuid::new_v4(),"action":{"kind":"release_artist_set","release_id":release,"artist":"My album credit"}},
        {"id":Uuid::new_v4(),"action":{"kind":"recording_artist_set","recording_id":recording,"artist":"My performer"}}
    ]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", batch, Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let detail = call(&app, "GET", &path, Value::Null, Some(&token)).await.1;
    assert_eq!(detail["title"]["value"], "My album");
    assert_eq!(detail["artist"]["value"], "My album credit");
    assert_eq!(detail["artist"]["inherited"], original["artist"]["value"]);
    assert_eq!(detail["observations"], original["observations"]);
    let invalid = json!({"device_id":"metadata-client","operations":[
        {"id":Uuid::new_v4(),"action":{"kind":"release_title_set","release_id":release,"title":"Rolled back album"}},
        {"id":Uuid::new_v4(),"action":{"kind":"release_artist_set","release_id":release,"artist":"Invalid\ncredit"}}
    ]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", invalid, Some(&token))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, "GET", &path, Value::Null, Some(&token)).await.1,
        detail
    );
    let track = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{recording}/metadata"),
        Value::Null,
        Some(&token),
    )
    .await
    .1;
    assert_eq!(track["artist"]["value"], "My performer");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/albums/{release}/details"))
                .header("authorization", format!("Bearer {token}"))
                .header("x-inertia", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["component"], "AlbumDetailsPage");
    assert_eq!(page["props"]["detail"], detail);
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
    for action in [
        json!({"kind":"release_title_set","release_id":release,"title":"Forbidden"}),
        json!({"kind":"release_artist_set","release_id":release,"artist":"Forbidden"}),
        json!({"kind":"release_title_inherit","release_id":release}),
        json!({"kind":"release_artist_inherit","release_id":release}),
    ] {
        let batch = json!({"device_id":"bob-metadata-client","operations":[{"id":Uuid::new_v4(),"action":action}]});
        assert_eq!(
            call(&app, "POST", "/api/v1/sync", batch, Some(bob_token))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
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
    let inherited = call(&app, "GET", &path, Value::Null, Some(bob_token))
        .await
        .1;
    assert_eq!(inherited["title"]["value"], original["title"]["value"]);
    assert_eq!(inherited["artist"]["value"], original["artist"]["value"]);
    assert!(inherited["title"]["operation_id"].is_null());
    assert!(!inherited.to_string().contains("My album"));
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
