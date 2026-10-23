use super::support::{admin, app, call};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn inertia_serves_html_json_partial_props_and_asset_version_conflicts() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/library")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["vary"], "X-Inertia");
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("data-page=\"app\""));
    assert!(html.contains("\"native_playback\":false"));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/library")
                .header("authorization", format!("Bearer {token}"))
                .header("x-inertia", "true")
                .header("x-inertia-partial-component", "CatalogPage")
                .header("x-inertia-partial-data", "recordings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-inertia"], "true");
    let page: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["component"], "CatalogPage");
    assert_eq!(
        page["props"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["errors", "recordings"]
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/library")
                .header("authorization", format!("Bearer {token}"))
                .header("x-inertia", "true")
                .header("x-inertia-version", "outdated")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["x-inertia-location"], "/library");
}

#[tokio::test]
async fn inertia_evaluates_only_requested_page_data() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    for (path, component, field) in [
        (
            format!("/library?q={}", "x".repeat(513)),
            "CatalogPage",
            "query",
        ),
        ("/playlists/nonexistent".into(), "PlaylistPage", "user"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .header("x-inertia", "true")
                    .header("x-inertia-partial-component", component)
                    .header("x-inertia-partial-data", field)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/settings?q={}", "x".repeat(513)),
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::OK
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/library")
                .header("x-inertia", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let page: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["clearHistory"], true);
}

#[tokio::test]
async fn inertia_pages_only_include_the_data_for_their_screen() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let shared = vec![
        "errors",
        "libraries",
        "native_playback",
        "playlists",
        "user",
    ];
    for (path, component, fields) in [
        (
            "/library",
            "CatalogPage",
            vec![
                "errors",
                "libraries",
                "native_playback",
                "playlists",
                "query",
                "recordings",
                "user",
            ],
        ),
        (
            "/likes",
            "LikesPage",
            vec![
                "errors",
                "libraries",
                "native_playback",
                "playlists",
                "query",
                "recordings",
                "user",
            ],
        ),
        (
            "/playlists",
            "PlaylistIndexPage",
            vec![
                "connections",
                "errors",
                "libraries",
                "native_playback",
                "playlists",
                "service_names",
                "user",
            ],
        ),
        (
            "/history",
            "HistoryPage",
            vec![
                "activity",
                "errors",
                "libraries",
                "native_playback",
                "playlists",
                "user",
            ],
        ),
        ("/settings", "SettingsPage", shared),
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
        let page: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(page["component"], component);
        assert_eq!(
            page["props"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            fields
        );
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/library")
                .header("x-inertia", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let page: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["component"], "LoginPage");
    assert_eq!(page["props"], json!({"errors":{}, "needs_setup":false}));
    assert_eq!(page["clearHistory"], true);
}

#[tokio::test]
async fn api_contract_is_published_without_authentication() {
    let (_dir, app) = app().await;
    let (status, contract) = call(&app, "GET", "/api/v1/openapi.json", Value::Null, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(contract["openapi"], "3.1.0");
    let mut operation_ids = std::collections::HashSet::new();
    for item in contract["paths"].as_object().unwrap().values() {
        for operation in item.as_object().unwrap().values() {
            if let Some(id) = operation["operationId"].as_str() {
                assert!(
                    operation_ids.insert(id),
                    "Operation IDs must be unique: {id}"
                );
            }
        }
    }
    assert!(contract["paths"]["/api/v1/sync"]["post"].is_object());
    assert_eq!(
        contract,
        serde_json::to_value(hito_server::openapi()).unwrap()
    );
    for schema in [
        "SyncReceipt",
        "RecordingPage",
        "SharedPageProps",
        "LoginPageProps",
        "CatalogPageProps",
        "PlaylistIndexPageProps",
        "PlaylistPageProps",
        "HistoryPageProps",
        "SettingsPageProps",
    ] {
        assert!(
            contract["components"]["schemas"][schema].is_object(),
            "Missing schema: {schema}"
        );
    }
}
