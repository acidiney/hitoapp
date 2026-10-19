#[path = "../../hito-core/tests/fixtures/community_ranking_library.rs"]
mod library;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/fixtures/community_rankings.rs"]
#[allow(dead_code)]
mod remote;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{CommunityPreferences, CommunityRankingPreferences, Credentials};
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, time::Duration};
use tokio::time::timeout;
use tower::ServiceExt;

const PREFERENCES: &str = "/api/v1/community/ranking-preferences";

#[tokio::test]
async fn explicit_account_ranking_consent_reorders_playback_sources_through_authenticated_http() {
    let primary = provider::ProviderServer::start().await;
    let candidate = provider::ProviderServer::start().await;
    let ranking = remote::RankingServer::start().await;
    let result = timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(&primary, &candidate, &ranking)).catch_unwind(),
    )
    .await;
    primary.shutdown().await;
    candidate.shutdown().await;
    ranking.close().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("The owned HTTP ranking workflow must finish within its deadline"),
    }
}

async fn exercise(
    primary: &provider::ProviderServer,
    candidate: &provider::ProviderServer,
    ranking: &remote::RankingServer,
) {
    let owned = library::Library::prepare(primary, candidate, &ranking.url).await;
    let hito = &owned.hito;
    let session = hito.identity.login(credentials("owner")).await.unwrap();
    let account = owned.actor.user().id.as_str();
    hito.identity
        .create_user(&owned.actor, credentials("ranking-peer"))
        .await
        .unwrap();
    let peer = hito
        .identity
        .login(credentials("ranking-peer"))
        .await
        .unwrap();
    let app = hito_server::router(hito.clone());
    let before = owned.private_state().await;
    let provider_before = provider_counts(primary, candidate).await;
    let disabled = json!(CommunityRankingPreferences::default());
    let baseline = serde_json::to_value(&owned.baseline).unwrap();
    let source_path = format!("/api/v1/recordings/{}/playback-sources", owned.recording.id);
    let legacy_path = format!("/api/v1/recordings/{}/sources", owned.recording.id);
    let read = |path: &str| {
        let path = path.to_owned();
        let app = &app;
        let token = &session.token;
        async move { request(app, "GET", &path, Some(token), account, "", StatusCode::OK).await }
    };
    assert_eq!(read(PREFERENCES).await, disabled);
    assert!(read(&source_path).await == baseline);
    ranking.assert_no_requests().await;
    for path in [PREFERENCES, source_path.as_str()] {
        request(
            &app,
            "GET",
            path,
            None,
            account,
            "",
            StatusCode::UNAUTHORIZED,
        )
        .await;
        let denied = request(
            &app,
            "GET",
            path,
            Some(&session.token),
            &peer.user.id,
            "",
            StatusCode::FORBIDDEN,
        )
        .await;
        assert!(denied.get("use_source_rankings").is_none());
    }
    let saved = request(
        &app,
        "PUT",
        PREFERENCES,
        Some(&session.token),
        account,
        &json!({"revision": 0,"use_source_rankings": true}).to_string(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        saved,
        json!(CommunityRankingPreferences {
            revision: 1,
            use_source_rankings: true
        })
    );
    assert_eq!(
        read("/api/v1/community/preferences").await,
        json!(CommunityPreferences::default())
    );
    assert_eq!(
        request(
            &app,
            "GET",
            PREFERENCES,
            Some(&peer.token),
            &peer.user.id,
            "",
            StatusCode::OK
        )
        .await,
        disabled
    );
    ranking.assert_no_requests().await;
    let expected: Vec<_> = owned
        .baseline
        .iter()
        .map(library::public_reference)
        .collect();
    ranking
        .reply(json!({"ranked": [
            {"source": expected[1],"successful_reporters": 7},
            {"source": expected[0],"successful_reporters": 1},
        ]}))
        .await;
    let ranked = read(&source_path).await;
    assert!(
        ranked == serde_json::to_value(owned.baseline.iter().rev().collect::<Vec<_>>()).unwrap()
    );
    ranking
        .assert_one_public_request(json!({"candidates": expected}))
        .await;
    assert!(read(&legacy_path).await == baseline);
    request(
        &app,
        "GET",
        &source_path,
        Some(&peer.token),
        &peer.user.id,
        "",
        StatusCode::NOT_FOUND,
    )
    .await;
    for invalid in [
        json!({"revision": 1,"use_source_rankings": true,"user_id": peer.user.id}).to_string(),
        json!({"revision": 1,"use_source_rankings": "true"}).to_string(),
    ] {
        request(
            &app,
            "PUT",
            PREFERENCES,
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
        PREFERENCES,
        Some(&session.token),
        account,
        "{",
        StatusCode::BAD_REQUEST,
    )
    .await;
    request(
        &app,
        "PUT",
        PREFERENCES,
        Some(&session.token),
        account,
        &json!({"revision": 0,"use_source_rankings": false}).to_string(),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(read(PREFERENCES).await, saved);
    assert_eq!(
        read("/api/v1/community/preferences").await,
        json!(CommunityPreferences::default())
    );
    request(
        &app,
        "POST",
        "/api/v1/logout",
        Some(&session.token),
        account,
        "",
        StatusCode::NO_CONTENT,
    )
    .await;
    for path in [PREFERENCES, source_path.as_str()] {
        request(
            &app,
            "GET",
            path,
            Some(&session.token),
            account,
            "",
            StatusCode::UNAUTHORIZED,
        )
        .await;
    }
    ranking
        .assert_one_public_request(json!({"candidates": expected}))
        .await;
    assert!(primary.requests("v1/audio").await.is_empty());
    assert!(candidate.requests("v1/audio").await.is_empty());
    assert_eq!(provider_counts(primary, candidate).await, provider_before);
    assert_eq!(owned.private_state().await, before);
    owned.assert_original();
}

async fn provider_counts(
    primary: &provider::ProviderServer,
    candidate: &provider::ProviderServer,
) -> Vec<usize> {
    let mut counts = Vec::new();
    for path in [
        "v1/manifest",
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        hito_plugin_sdk::PREPARE_SOURCE_PATH,
        "v1/audio",
    ] {
        for provider in [primary, candidate] {
            counts.push(provider.requests(path).await.len());
        }
    }
    counts
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    account: &str,
    body: &str,
    expected_status: StatusCode,
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
    assert_eq!(response.status(), expected_status);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers().contains_key("x-request-id"));
    let bytes = timeout(
        Duration::from_secs(2),
        to_bytes(response.into_body(), 65_536),
    )
    .await
    .expect("The owned HTTP body must finish within its deadline")
    .unwrap();
    if expected_status == StatusCode::NO_CONTENT {
        assert!(bytes.is_empty());
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("The public ranking route must return bounded JSON")
    }
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "private-ranking-account-password".into(),
    }
}
