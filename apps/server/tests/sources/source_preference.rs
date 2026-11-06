use hito_core::test_support::community_ranking_library as library;
use hito_core::test_support::community_rankings as remote;
use hito_core::test_support::content_service as provider;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_core::Credentials;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::timeout;
use tower::ServiceExt;

const PATH: &str = "/api/v1/source-preferences";

#[tokio::test]
async fn an_account_ranks_its_providers_through_authenticated_http_without_touching_another_account()
 {
    let primary = provider::ProviderServer::start().await;
    let candidate = provider::ProviderServer::start().await;
    let ranking = remote::RankingServer::start().await;
    let owned = library::Library::prepare(&primary, &candidate, &ranking.url).await;
    let hito = &owned.hito;
    let session = hito.identity.login(credentials("owner")).await.unwrap();
    let account = owned.actor.user().id.as_str();
    hito.identity
        .create_user(&owned.actor, credentials("peer"))
        .await
        .unwrap();
    let peer = hito.identity.login(credentials("peer")).await.unwrap();
    let settings = hito.plugins.settings(&owned.actor).await.unwrap();
    let installation = |manifest: &str| {
        settings
            .installations
            .iter()
            .find(|installation| installation.manifest.id == manifest)
            .unwrap()
            .id
            .clone()
    };
    let (youtube, jamendo) = (
        installation("org.hito.youtube"),
        installation("org.hito.jamendo"),
    );
    let app = hito_server::router(hito.clone());
    let sources_path = format!("/api/v1/recordings/{}/playback-sources", owned.recording.id);
    let labels = |sources: Value| -> Vec<String> {
        sources
            .as_array()
            .unwrap()
            .iter()
            .map(|source| source["label"].as_str().unwrap().to_owned())
            .collect()
    };
    request(
        &app,
        "GET",
        PATH,
        None,
        account,
        "",
        StatusCode::UNAUTHORIZED,
    )
    .await;
    request(
        &app,
        "GET",
        PATH,
        Some(&session.token),
        &peer.user.id,
        "",
        StatusCode::FORBIDDEN,
    )
    .await;
    let fresh = request(
        &app,
        "GET",
        PATH,
        Some(&session.token),
        account,
        "",
        StatusCode::OK,
    )
    .await;
    assert_eq!(fresh["revision"], 0);
    assert_eq!(fresh["fallback"], true);
    assert_eq!(fresh["entries"][0], json!({"kind": "local_files"}));
    assert_eq!(fresh["entries"][1], json!({"kind": "saved_copies"}));
    assert_eq!(fresh["entries"].as_array().unwrap().len(), 4);
    let order = |first: &str, second: &str, revision: i64, fallback: bool| {
        json!({"revision": revision, "fallback": fallback, "entries": [
            {"kind": "local_files"}, {"kind": "saved_copies"},
            {"kind": "provider", "installation_id": first},
            {"kind": "provider", "installation_id": second},
        ]})
        .to_string()
    };
    let saved = request(
        &app,
        "PUT",
        PATH,
        Some(&session.token),
        account,
        &order(&jamendo, &youtube, 0, false),
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["fallback"], false);
    assert_eq!(
        saved["entries"][2],
        json!({"kind": "provider", "installation_id": jamendo})
    );
    let listed = request(
        &app,
        "GET",
        &sources_path,
        Some(&session.token),
        account,
        "",
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        labels(listed),
        ["private-jamendo-label", "private-youtube-label"]
    );
    request(
        &app,
        "PUT",
        PATH,
        Some(&session.token),
        account,
        &order(&youtube, &jamendo, 1, true),
        StatusCode::OK,
    )
    .await;
    let listed = request(
        &app,
        "GET",
        &sources_path,
        Some(&session.token),
        account,
        "",
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        labels(listed),
        ["private-youtube-label", "private-jamendo-label"]
    );
    request(
        &app,
        "PUT",
        PATH,
        Some(&session.token),
        account,
        &order(&youtube, &jamendo, 1, true),
        StatusCode::CONFLICT,
    )
    .await;
    let theirs = request(
        &app,
        "GET",
        PATH,
        Some(&peer.token),
        &peer.user.id,
        "",
        StatusCode::OK,
    )
    .await;
    assert_eq!(theirs["revision"], 0);
    assert_eq!(theirs["entries"].as_array().unwrap().len(), 2);
    request(
        &app,
        "PUT",
        PATH,
        Some(&peer.token),
        &peer.user.id,
        &order(&youtube, &jamendo, 0, false),
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    request(
        &app,
        "GET",
        &sources_path,
        Some(&peer.token),
        &peer.user.id,
        "",
        StatusCode::NOT_FOUND,
    )
    .await;
    for invalid in [
        json!({"revision": 2, "fallback": true, "entries": [], "user_id": peer.user.id})
            .to_string(),
        json!({"revision": 2, "fallback": "yes", "entries": []}).to_string(),
        json!({"revision": 2, "fallback": true, "entries": [{"kind": "provider"}]}).to_string(),
        json!({"revision": 2, "fallback": true, "entries": [{"kind": "relay"}]}).to_string(),
    ] {
        request(
            &app,
            "PUT",
            PATH,
            Some(&session.token),
            account,
            &invalid,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    }
    request(
        &app,
        "PUT",
        PATH,
        Some(&session.token),
        account,
        "{",
        StatusCode::BAD_REQUEST,
    )
    .await;
    let kept = request(
        &app,
        "GET",
        PATH,
        Some(&session.token),
        account,
        "",
        StatusCode::OK,
    )
    .await;
    assert_eq!(kept["revision"], 2);
    primary.shutdown().await;
    candidate.shutdown().await;
    ranking.close().await;
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    account: &str,
    body: &str,
    expected: StatusCode,
) -> Value {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "http://localhost:8080")
        .header("x-hito-request", "1")
        .header("x-hito-account", account)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("cookie", format!("hito_session={token}"));
    }
    let response = timeout(
        Duration::from_secs(2),
        app.clone()
            .oneshot(request.body(Body::from(body.to_owned())).unwrap()),
    )
    .await
    .expect("The owned HTTP response must finish within its deadline")
    .unwrap();
    assert_eq!(response.status(), expected);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let bytes = timeout(
        Duration::from_secs(2),
        to_bytes(response.into_body(), 65_536),
    )
    .await
    .expect("The owned HTTP body must finish within its deadline")
    .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "private-ranking-account-password".into(),
    }
}
