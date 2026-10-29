use super::support::{app, call, indexed_app};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn page(
    app: &Router,
    path: &str,
    token: Option<&str>,
    partial: Option<(&str, &str)>,
) -> Value {
    let mut request = Request::builder().uri(path).header("x-inertia", "true");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some((component, keys)) = partial {
        request = request
            .header("x-inertia-partial-component", component)
            .header("x-inertia-partial-data", keys);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn home_page_reads_the_listening_summary_only_when_requested() {
    let (_dir, app, token, recording) = indexed_app().await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/sync",
        json!({"device_id":"home-test","operations":[
            {"id":uuid::Uuid::new_v4(),"action":{"kind":"like_set","recording_id":recording,"liked":true}}
        ]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let full = page(&app, "/home", Some(&token), None).await;
    assert_eq!(full["component"], "HomePage");
    let home = &full["props"]["home"];
    assert_eq!(home["has_music"], true);
    assert_eq!(home["liked_count"], 1);
    assert_eq!(home["recent_albums"], json!([]));
    assert_eq!(home["unheard"], json!([]));

    let partial = page(&app, "/home", Some(&token), Some(("HomePage", "playlists"))).await;
    assert!(partial["props"].get("home").is_none());
    assert!(partial["props"].get("playlists").is_some());

    let anonymous = page(&app, "/home", None, None).await;
    assert_eq!(anonymous["component"], "LoginPage");
}

#[tokio::test]
async fn search_page_groups_library_results_and_lists_nothing_without_a_query() {
    let (_dir, app, token, _) = indexed_app().await;
    let found = page(&app, "/search?q=Fixture", Some(&token), None).await;
    assert_eq!(found["component"], "SearchPage");
    let props = &found["props"];
    assert_eq!(props["query"], "Fixture");
    assert_eq!(props["recordings"]["items"][0]["title"], "Fixture");
    assert!(props["releases"]["items"].is_array());
    assert!(props["artists"]["items"].is_array());
    assert!(props["connections"].is_array());
    assert!(props["service_names"].is_object());
    assert!(props["recordings"]["items"].as_array().unwrap().len() <= 8);

    let empty = page(&app, "/search", Some(&token), None).await;
    assert_eq!(empty["props"]["query"], "");
    assert_eq!(empty["props"]["recordings"]["items"], json!([]));
    assert_eq!(empty["props"]["recordings"]["total"], 0);
    assert_eq!(empty["props"]["releases"]["items"], json!([]));
    assert_eq!(empty["props"]["artists"]["items"], json!([]));

    let partial = page(
        &app,
        "/search?q=Fixture",
        Some(&token),
        Some(("SearchPage", "recordings")),
    )
    .await;
    assert!(partial["props"].get("releases").is_none());
    assert!(partial["props"].get("recordings").is_some());
}

#[tokio::test]
async fn home_is_its_own_route_and_the_library_routes_are_unchanged() {
    let (_dir, app) = app().await;
    let token = super::support::admin(&app).await;
    for (path, component) in [
        ("/", "CatalogPage"),
        ("/library", "CatalogPage"),
        ("/home", "HomePage"),
        ("/search", "SearchPage"),
        ("/discover", "DiscoverPage"),
    ] {
        assert_eq!(
            page(&app, path, Some(&token), None).await["component"],
            component
        );
    }
}
