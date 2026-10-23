use super::provider_fixture::ProviderServer;
use super::support::{admin, app, call};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn playlist_index_returns_private_accepted_connections_and_selective_named_props() {
    let provider = ProviderServer::start().await;
    let manifest = json!({
        "id": "org.hito.playlist-page-fixture",
        "name": "Playlist fixture",
        "protocol_version": 1,
        "capabilities": ["playlists"]
    });
    provider.reply("v1/manifest", manifest.clone()).await;
    provider
        .reply(
            "v1/playlists/detail",
            json!({
                "id": "playlist/canonical-page-fixture",
                "name": "Copied selection",
                "snapshot": "snapshot-one",
                "total_count": 0,
                "attribution": {"label": "Fixture owner"}
            }),
        )
        .await;
    provider
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": "playlist/canonical-page-fixture",
                "snapshot": "snapshot-one",
                "items": [],
                "next_cursor": null
            }),
        )
        .await;
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let (status, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Playlist collection", "base_url": provider.url}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({
            "service_id": service["id"],
            "service_revision": service["revision"],
            "bearer_token": "private-playlist-page-credential"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let private_reference = "https://provider.invalid/private-playlist-page-reference";
    let (status, _) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({
            "request_id": uuid::Uuid::new_v4(),
            "installation_revision": installation["revision"],
            "playlist_reference": private_reference
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut changed = manifest.clone();
    changed["capabilities"] = json!(["search"]);
    provider.reply("v1/manifest", changed).await;
    let (status, review) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/refresh",
            installation["id"].as_str().unwrap()
        ),
        json!({"revision": installation["revision"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["kind"], "review_required");
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "playlist-listener", "password": "playlist listener password"}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, listener) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "playlist-listener", "password": "playlist listener password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for (account, count, partial) in [
        (token.as_str(), 1, None),
        (listener["token"].as_str().unwrap(), 0, None),
        (token.as_str(), 1, Some("connections")),
        (token.as_str(), 1, Some("service_names")),
        (token.as_str(), 1, Some("playlists")),
    ] {
        let mut request = Request::builder()
            .uri("/playlists")
            .header("authorization", format!("Bearer {account}"))
            .header("x-inertia", "true");
        if let Some(field) = partial {
            request = request
                .header("x-inertia-partial-component", "PlaylistIndexPage")
                .header("x-inertia-partial-data", field);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let page: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(page["component"], "PlaylistIndexPage");
        let props = &page["props"];
        if partial.is_none_or(|field| field == "connections") {
            let connections = props["connections"]
                .as_array()
                .expect("Requested connections must be present");
            assert_eq!(connections.len(), count);
            if count > 0 {
                assert_eq!(connections[0]["id"], installation["id"]);
                assert_eq!(connections[0]["manifest"], manifest);
                assert_eq!(connections[0]["revision"], installation["revision"]);
            }
        } else {
            assert!(props.get("connections").is_none());
        }
        if partial.is_none_or(|field| field == "service_names") {
            let names = props["service_names"]
                .as_object()
                .expect("Requested service names must be present");
            assert_eq!(names.len(), count);
            if count > 0 {
                assert_eq!(
                    names[service["id"].as_str().unwrap()],
                    "Playlist collection"
                );
            }
        } else {
            assert!(props.get("service_names").is_none());
        }
        if let Some(field) = partial {
            let mut expected = vec!["errors", field];
            expected.sort_unstable();
            assert_eq!(
                props
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let output = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!output.contains(&provider.url));
        assert!(!output.contains("base_url"));
        assert!(!output.contains("private-playlist-page-credential"));
        assert!(!output.contains(private_reference));
        assert!(!output.contains(&token));
        assert!(!output.contains(listener["token"].as_str().unwrap()));
    }
}
