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

    let full = page(&app, "/", Some(&token), None).await;
    assert_eq!(full["component"], "HomePage");
    let home = &full["props"]["home"];
    assert_eq!(home["has_music"], true);
    assert_eq!(home["liked_count"], 1);
    assert_eq!(home["recent_albums"], json!([]));
    assert_eq!(home["unheard"], json!([]));
    assert!(home["recently_added"].is_array());
    assert_eq!(full["props"]["artists"]["items"], json!([]));
    assert_eq!(full["props"]["artists"]["limit"], 12);

    let partial = page(&app, "/", Some(&token), Some(("HomePage", "playlists"))).await;
    assert!(partial["props"].get("home").is_none());
    assert!(partial["props"].get("artists").is_none());
    assert!(partial["props"].get("playlists").is_some());

    let anonymous = page(&app, "/", None, None).await;
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
async fn root_and_home_share_home_while_library_keeps_catalog() {
    let (_dir, app) = app().await;
    let setup = page(&app, "/", None, None).await;
    assert_eq!(setup["component"], "LoginPage");
    assert_eq!(setup["props"]["needs_setup"], true);
    assert_eq!(setup["clearHistory"], true);
    let token = super::support::admin(&app).await;
    for (path, component) in [
        ("/", "HomePage"),
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
    let anonymous = page(&app, "/", None, None).await;
    assert_eq!(anonymous["component"], "LoginPage");
    assert_eq!(anonymous["props"]["needs_setup"], false);
    assert_eq!(anonymous["clearHistory"], true);
}

#[tokio::test]
async fn a_locked_session_gets_the_lock_page_for_home_and_search() {
    let (_dir, app, token, _) = indexed_app().await;
    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password":"administrator test password","pin":"1234","idle_minutes":15}),
        Some(&token),
    )
    .await;
    assert!(status.is_success(), "{status}");
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    assert!(status.is_success(), "{status}");
    for path in ["/", "/library", "/home", "/search?q=Fixture"] {
        let locked = page(&app, path, Some(&token), None).await;
        assert_eq!(locked["component"], "LockPage", "{path}");
        assert_eq!(locked["props"]["locked"], true);
        assert!(locked["props"].get("home").is_none());
        assert!(locked["props"].get("recordings").is_none());
    }
}
