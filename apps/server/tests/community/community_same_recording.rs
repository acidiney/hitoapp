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
async fn a_listener_suggestion_reaches_the_community_service_with_the_reporter_token() {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let owner = &library.accounts[0];
    let youtube = owner.attach_youtube(&hito, &owner.actor).await;
    let session = hito
        .identity
        .login(fixture::credentials("recovery-admin"))
        .await
        .unwrap();
    let app = hito_server::router(hito.clone());
    let account = owner.actor.user().id.as_str();
    let path = format!("/api/v1/sources/{}/same-recording", owner.source.id);
    let submission = Uuid::new_v4();
    let body =
        json!({"submission_id": submission, "other_source_id": youtube.source.id}).to_string();
    assert_eq!(
        request(&app, "POST", &path, None, account, &body).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "POST", &path, Some(&session.token), account, &body).await,
        (StatusCode::OK, json!("ignored"))
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/v1/community/preferences",
            Some(&session.token),
            account,
            &json!({"revision": 0, "playback_reporting": false, "metadata_corrections": true})
                .to_string()
        )
        .await
        .0,
        StatusCode::OK
    );
    let body =
        json!({"submission_id": submission, "other_source_id": youtube.source.id}).to_string();
    assert_eq!(
        request(&app, "POST", &path, Some(&session.token), account, &body).await,
        (StatusCode::ACCEPTED, json!("queued"))
    );
    assert!(
        hito.community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    let delivered = remote
        .requests()
        .await
        .into_iter()
        .filter(|request| request.path == "/v1/corrections")
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0].authorization.as_deref(),
        Some(format!("Bearer {}", community::REPORTER_TOKEN).as_str())
    );
    assert_eq!(delivered[0].body["submission_id"], json!(submission));
    assert_eq!(
        request(&app, "POST", &path, Some(&session.token), account, &body).await,
        (StatusCode::OK, json!("already_accepted"))
    );
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
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
