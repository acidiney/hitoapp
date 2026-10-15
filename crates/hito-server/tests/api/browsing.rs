use super::support::{call, indexed_app};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn artist_and_album_pages_share_the_authorized_json_catalog() {
    let (_dir, app, token, _) = indexed_app().await;
    for (resource, page, index_component, detail_component) in [
        ("releases", "albums", "AlbumIndexPage", "AlbumPage"),
        ("artists", "artists", "ArtistIndexPage", "ArtistPage"),
    ] {
        let (status, index) = call(
            &app,
            "GET",
            &format!("/api/v1/{resource}?limit=1"),
            Value::Null,
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(index["total"], 1);
        let id = index["items"][0]["id"].as_str().unwrap();
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("/api/v1/{resource}/{id}"),
                Value::Null,
                Some(&token)
            )
            .await
            .0,
            StatusCode::OK
        );
        for (path, component) in [
            (format!("/{page}"), index_component),
            (format!("/{page}/{id}"), detail_component),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("authorization", format!("Bearer {token}"))
                        .header("x-inertia", "true")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let data: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(data["component"], component);
            assert!(
                data["props"][if component.ends_with("IndexPage") {
                    resource
                } else {
                    "detail"
                }]
                .is_object()
            );
        }
    }
    let (_, listener) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"listener","password":"listener private password"}),
        Some(&token),
    )
    .await;
    assert!(listener["id"].is_string());
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"listener","password":"listener private password"}),
        None,
    )
    .await;
    for resource in ["releases", "artists"] {
        let index = call(
            &app,
            "GET",
            &format!("/api/v1/{resource}"),
            Value::Null,
            Some(&token),
        )
        .await
        .1;
        let id = index["items"][0]["id"].as_str().unwrap();
        let listener = session["token"].as_str().unwrap();
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("/api/v1/{resource}"),
                Value::Null,
                Some(listener)
            )
            .await
            .1["total"],
            0
        );
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("/api/v1/{resource}/{id}"),
                Value::Null,
                Some(listener)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("/api/v1/{resource}/{id}"),
                Value::Null,
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn release_artwork_is_private_and_requires_library_membership() {
    let (dir, app, token, _) = indexed_app().await;
    let cover = include_bytes!("../fixtures/cover.png");
    std::fs::write(dir.path().join("music/cover.png"), cover).unwrap();
    let index = call(&app, "GET", "/api/v1/releases", Value::Null, Some(&token))
        .await
        .1;
    let id = index["items"][0]["id"].as_str().unwrap();
    let path = format!("/api/v1/releases/{id}/artwork");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        cover
    );
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"listener","password":"listener private password"}),
        Some(&token),
    )
    .await;
    let session = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"listener","password":"listener private password"}),
        None,
    )
    .await
    .1;
    assert_eq!(
        call(&app, "GET", &path, Value::Null, session["token"].as_str())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", &path, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn artist_tracks_and_albums_have_independent_json_and_inertia_offsets() {
    let (_dir, app, token, recording) = indexed_app().await;
    let (_, artists) = call(&app, "GET", "/api/v1/artists", Value::Null, Some(&token)).await;
    let artist = artists["items"][0]["id"].as_str().unwrap();
    let (_, filtered) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings?artist={artist}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["items"][0]["id"], recording);
    let (_, missing) = call(
        &app,
        "GET",
        "/api/v1/recordings?artist=missing",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(missing["total"], 0);
    let query = "offset=0&limit=1&recording_offset=1";
    let (_, json) = call(
        &app,
        "GET",
        &format!("/api/v1/artists/{artist}?{query}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(json["releases"]["offset"], 0);
    assert_eq!(json["releases"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(json["recordings"]["offset"], 1);
    assert!(json["recordings"]["items"].as_array().unwrap().is_empty());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/artists/{artist}?{query}"))
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
    assert_eq!(page["props"]["detail"], json);
}
