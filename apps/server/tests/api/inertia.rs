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
        vec!["errors", "files", "recordings"]
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
        "lock",
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
                "files",
                "libraries",
                "lock",
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
                "lock",
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
                "lock",
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
                "history_enabled",
                "libraries",
                "lock",
                "native_playback",
                "playlists",
                "user",
            ],
        ),
        ("/settings", "SettingsPage", shared.clone()),
        ("/queue", "QueuePage", shared),
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
        "QueuePageProps",
    ] {
        assert!(
            contract["components"]["schemas"][schema].is_object(),
            "Missing schema: {schema}"
        );
    }
}

async fn get(
    app: &axum::Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, String, axum::http::HeaderMap) {
    let mut request = Request::builder().uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (status, headers) = (response.status(), response.headers().clone());
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    (status, body, headers)
}

#[tokio::test]
async fn unknown_paths_answer_a_not_found_page_while_api_paths_keep_json() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let bearer = format!("Bearer {token}");

    let (status, body, headers) = get(
        &app,
        "/no/such/page",
        &[("authorization", &bearer), ("x-inertia", "true")],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers["x-inertia"], "true");
    let page: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(page["component"], "NotFoundPage");

    let (status, body, _) = get(&app, "/no/such/page", &[("authorization", &bearer)]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("data-page=\"app\"") && body.contains("NotFoundPage"));

    let (status, body, _) = get(&app, "/no/such/page", &[]).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "A signed-out visitor learns nothing more"
    );
    assert!(body.contains("NotFoundPage"));

    let (status, body, headers) =
        get(&app, "/api/v1/no-such-route", &[("authorization", &bearer)]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert!(serde_json::from_str::<Value>(&body).unwrap()["error"].is_string());

    let (status, _, _) = get(&app, "/library", &[("authorization", &bearer)]).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn search_pages_can_carry_chosen_artists() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let bearer = format!("Bearer {token}");
    let (status, body, _) = get(
        &app,
        "/search?q=nova",
        &[
            ("authorization", &bearer),
            ("x-inertia", "true"),
            ("x-inertia-partial-component", "SearchPage"),
            ("x-inertia-partial-data", "artist_groups"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let page: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(page["props"]["artist_groups"]["items"], json!([]));
    assert_eq!(page["props"]["artist_groups"]["total"], 0);
}

#[tokio::test]
async fn asset_like_and_api_misses_answer_without_pages_or_version_checks() {
    let (_dir, app) = app().await;
    let (status, body, headers) = get(&app, "/favicon.ico", &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !body.contains("data-page"),
        "No page is rendered for a file-like path"
    );
    assert!(headers.get("vary").is_none() || !body.contains("NotFoundPage"));

    let (status, body, headers) = get(
        &app,
        "/api/v1/missing",
        &[("x-inertia", "true"), ("x-inertia-version", "stale")],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
}
