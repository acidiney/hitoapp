use super::provider_fixture::ProviderServer;
use super::support::{admin, call, services};
use axum::{
    Router,
    extract::Query,
    http::{StatusCode, Uri},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};

#[tokio::test]
async fn plugin_consent_attempt_is_durable_and_bound_to_the_original_session() {
    let directory = tempfile::tempdir().unwrap();
    let app = configured_app(directory.path()).await;
    let original_session = admin(&app).await;
    let (_provider, installation) = install_authorized_plugin(&app, &original_session).await;
    let installation_id = installation["id"].as_str().unwrap();
    let begin_path = format!("/api/v1/plugins/{installation_id}/authorization");
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (status, attempt) = call(
        &app,
        "POST",
        &begin_path,
        json!({"revision": installation["revision"]}),
        Some(&original_session),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let attempt_id = attempt["id"].as_str().unwrap();
    assert!(!attempt_id.is_empty());
    let expires_at = attempt["expires_at"].as_u64().unwrap();
    assert!((before + 1..=before + 600).contains(&expires_at));
    let authorization_url = attempt["authorization_url"].as_str().unwrap();
    let uri: Uri = authorization_url.parse().unwrap();
    assert_eq!(uri.scheme_str(), Some("https"));
    assert_eq!(uri.authority().unwrap().as_str(), "accounts.spotify.com");
    assert_eq!(uri.path(), "/authorize");
    let Query(query) = Query::<BTreeMap<String, String>>::try_from_uri(&uri).unwrap();
    assert_eq!(query.len(), 7);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], "owned-test-client");
    assert_eq!(
        query["redirect_uri"],
        "https://hito.test/api/v1/plugin-authorization/callback"
    );
    assert_eq!(query["scope"], "playlist-read-private");
    assert_eq!(query["code_challenge_method"], "S256");
    for (field, minimum, maximum) in [("state", 43, 128), ("code_challenge", 43, 43)] {
        let value = &query[field];
        assert!((minimum..=maximum).contains(&value.len()));
        assert!(
            value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
    }
    assert_eq!(
        attempt,
        json!({
            "id": attempt_id,
            "authorization_url": authorization_url,
            "expires_at": expires_at
        })
    );

    let status_path = format!("{begin_path}/{attempt_id}");
    let expected = json!({"id": attempt_id, "status": "pending", "expires_at": expires_at});
    let (status, pending) = call(
        &app,
        "GET",
        &status_path,
        Value::Null,
        Some(&original_session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending, expected);
    let (status, second_session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": "administrator test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second_session = second_session["token"].as_str().unwrap();
    assert_ne!(second_session, original_session);
    assert_eq!(
        call(&app, "GET", &status_path, Value::Null, Some(second_session))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob authorization test password"}),
        Some(&original_session),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, other_account) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "bob", "password": "bob authorization test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let other_account = other_account["token"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", &status_path, Value::Null, Some(other_account))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", &status_path, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );

    drop(app);
    let reopened = configured_app(directory.path()).await;
    let (status, retained) = call(
        &reopened,
        "GET",
        &status_path,
        Value::Null,
        Some(&original_session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retained, expected);
}

#[tokio::test]
async fn pending_consent_limit_is_durable_shared_between_sessions_and_private_to_each_account() {
    let directory = tempfile::tempdir().unwrap();
    let app = configured_app(directory.path()).await;
    let owner = admin(&app).await;
    let (_provider, installation) = install_authorized_plugin(&app, &owner).await;
    let installation_id = installation["id"].as_str().unwrap();
    let begin_path = format!("/api/v1/plugins/{installation_id}/authorization");
    let request = json!({"revision": installation["revision"]});
    let mut attempt_ids = BTreeSet::new();
    let mut states = BTreeSet::new();
    let mut challenges = BTreeSet::new();
    for _ in 0..8 {
        let (status, attempt) =
            call(&app, "POST", &begin_path, request.clone(), Some(&owner)).await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(attempt_ids.insert(attempt["id"].as_str().unwrap().to_owned()));
        let uri: Uri = attempt["authorization_url"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let Query(query) = Query::<BTreeMap<String, String>>::try_from_uri(&uri).unwrap();
        assert!(states.insert(query["state"].clone()));
        assert!(challenges.insert(query["code_challenge"].clone()));
    }
    assert_eq!(
        call(&app, "POST", &begin_path, request.clone(), Some(&owner))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, second_session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": "administrator test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second_session = second_session["token"].as_str().unwrap();
    assert_ne!(second_session, owner);
    assert_eq!(
        call(
            &app,
            "POST",
            &begin_path,
            request.clone(),
            Some(second_session)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );

    drop(app);
    let reopened = configured_app(directory.path()).await;
    assert_eq!(
        call(&reopened, "POST", &begin_path, request, Some(&owner))
            .await
            .0,
        StatusCode::CONFLICT
    );
    for attempt_id in &attempt_ids {
        let (status, pending) = call(
            &reopened,
            "GET",
            &format!("{begin_path}/{attempt_id}"),
            Value::Null,
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(pending["id"], attempt_id.as_str());
        assert_eq!(pending["status"], "pending");
    }
    let (status, _) = call(
        &reopened,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob consent admission password"}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, other_account) = call(
        &reopened,
        "POST",
        "/api/v1/login",
        json!({"username": "bob", "password": "bob consent admission password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let other_account = other_account["token"].as_str().unwrap();
    let (status, settings) = call(
        &reopened,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        Some(other_account),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["installations"], json!([]));
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let (status, other_installation) = call(
        &reopened,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        Some(other_account),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(other_installation["id"], installation["id"]);
    let other_begin_path = format!(
        "/api/v1/plugins/{}/authorization",
        other_installation["id"].as_str().unwrap()
    );
    let (status, other_attempt) = call(
        &reopened,
        "POST",
        &other_begin_path,
        json!({"revision": other_installation["revision"]}),
        Some(other_account),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let other_attempt_id = other_attempt["id"].as_str().unwrap();
    assert!(!attempt_ids.contains(other_attempt_id));
    let (status, pending) = call(
        &reopened,
        "GET",
        &format!("{other_begin_path}/{other_attempt_id}"),
        Value::Null,
        Some(other_account),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending["id"], other_attempt_id);
    assert_eq!(pending["status"], "pending");
}

async fn configured_app(directory: &std::path::Path) -> Router {
    let services = services(directory, &directory.join("music")).await;
    hito_server::router_with_config(
        services,
        hito_server::Config {
            origin: "https://hito.test".into(),
            ..Default::default()
        },
    )
}

async fn install_authorized_plugin(app: &Router, token: &str) -> (ProviderServer, Value) {
    let provider = ProviderServer::start().await;
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.spotify-authorization-fixture",
                "name": "Spotify authorization fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"],
                "authorization": {
                    "kind": "oauth2_pkce",
                    "client_id": "owned-test-client",
                    "authorization_endpoint": "https://accounts.spotify.com/authorize",
                    "scopes": ["playlist-read-private"]
                }
            }),
        )
        .await;
    let (status, service) = call(
        app,
        "POST",
        "/api/v1/plugins/services",
        json!({
            "name": "Spotify authorization fixture",
            "base_url": provider.url
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = call(
        app,
        "POST",
        "/api/v1/plugins",
        json!({
            "service_id": service["id"],
            "service_revision": service["revision"]
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    (provider, installation)
}
