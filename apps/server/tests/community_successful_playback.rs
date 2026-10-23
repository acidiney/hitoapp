#![cfg(unix)]

use hito_core::test_support::community_recovery as fixture;
use hito_core::test_support::community_service as community;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::future::pending;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn authenticated_source_claims_keep_consent_provenance_privacy_revocation_and_replay_in_core()
{
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let owner = &library.accounts[0];
    let peer = &library.accounts[1];
    let session = hito
        .identity
        .login(fixture::credentials("recovery-admin"))
        .await
        .unwrap();
    let app = hito_server::router(hito.clone());
    let path = format!("/api/v1/sources/{}/successful-playback", owner.source.id);
    let peer_path = format!("/api/v1/sources/{}/successful-playback", peer.source.id);
    let account = owner.actor.user().id.as_str();
    let before = [
        collection(owner.private_state(&hito).await),
        peer.private_state(&hito).await,
    ];
    let claim = || json!({"submission_id": Uuid::new_v4()}).to_string();
    assert_eq!(
        request(&app, "POST", &path, None, account, &claim())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            Some(&session.token),
            &peer.actor.user().id,
            &claim()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "POST", &path, Some(&session.token), account, &claim()).await,
        (StatusCode::OK, json!("ignored"))
    );
    save(&app, &session.token, account, 0, false).await;
    assert_eq!(
        request(&app, "POST", &path, Some(&session.token), account, &claim()).await,
        (StatusCode::OK, json!("ignored"))
    );
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    assert!(remote.requests().await.is_empty());
    save(&app, &session.token, account, 1, true).await;
    for body in ["{".to_owned(), json!({}).to_string(),
        json!({"submission_id": "private-input-canary"}).to_string(),
        json!({"submission_id": Uuid::new_v4(), "source": {"provider": "youtube", "recording_id": "private-input-canary"}}).to_string(),
        json!({"submission_id": "00000000-0000-1000-8000-000000000000"}).to_string()] {
        let (status, rejected) = request(&app, "POST", &path, Some(&session.token), account, &body).await;
        assert!(matches!(status, StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY));
        assert!(!rejected.to_string().contains("private-input-canary"));
    }
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            Some(&session.token),
            account,
            &"x".repeat(262_145)
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &peer_path,
            Some(&session.token),
            account,
            &claim()
        )
        .await,
        (StatusCode::OK, json!("ignored"))
    );
    let cancelled = claim();
    for _ in 0..2 {
        assert_eq!(
            request(
                &app,
                "POST",
                &path,
                Some(&session.token),
                account,
                &cancelled
            )
            .await,
            (StatusCode::ACCEPTED, json!("queued"))
        );
    }
    assert!(
        remote.requests().await.is_empty(),
        "HTTP admission must remain local"
    );
    save(&app, &session.token, account, 2, false).await;
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    save(&app, &session.token, account, 3, true).await;
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            Some(&session.token),
            account,
            &cancelled
        )
        .await,
        (StatusCode::OK, json!("ignored"))
    );
    assert_eq!(
        request(&app, "POST", &path, Some(&session.token), account, &claim())
            .await
            .0,
        StatusCode::ACCEPTED
    );
    hito.identity.logout(&session.token).await.unwrap();
    assert_eq!(
        request(&app, "POST", &path, Some(&session.token), account, &claim())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    assert!(remote.requests().await.is_empty());
    let session = hito
        .identity
        .login(fixture::credentials("recovery-admin"))
        .await
        .unwrap();
    let accepted = claim();
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            Some(&session.token),
            account,
            &accepted
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert!(
        hito.community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            Some(&session.token),
            account,
            &accepted
        )
        .await,
        (StatusCode::OK, json!("already_accepted"))
    );
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    let requests = remote.requests().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[0].body == json!({}));
    assert!(requests[0].authorization == Some(format!("Bearer {}", community::ADMIN_TOKEN)));
    assert!(requests[1].authorization == Some(format!("Bearer {}", community::REPORTER_TOKEN)));
    assert!(
        requests[1].body
            == json!({
                "submission_id": serde_json::from_str::<Value>(&accepted).unwrap()["submission_id"],
                "source": {"provider": "jamendo", "recording_id": "42"}, "outcome": "successful_playback"
            })
    );
    assert_eq!(collection(owner.private_state(&hito).await), before[0]);
    assert_eq!(peer.private_state(&hito).await, before[1]);
    library.assert_originals();
}

async fn save(app: &Router, token: &str, account: &str, revision: i64, playback: bool) {
    assert_eq!(request(app, "PUT", "/api/v1/community/preferences", Some(token), account,
        &json!({"revision": revision, "playback_reporting": playback, "metadata_corrections": true}).to_string()).await.0,
        StatusCode::OK);
}

fn collection(mut state: Value) -> Value {
    state.as_object_mut().unwrap().remove("preferences");
    state
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    account: &str,
    body: &str,
) -> (StatusCode, Value) {
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
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers().contains_key("x-request-id"));
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
