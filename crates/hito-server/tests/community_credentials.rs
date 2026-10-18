#![cfg(unix)]

#[path = "../../hito-core/tests/fixtures/community_service.rs"]
mod community;
#[path = "../../hito-core/tests/fixtures/community_recovery.rs"]
mod fixture;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{CommunityReportAdmission, SuccessfulPlayback, UpdateCommunityPreferences};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::future::pending;
use tower::ServiceExt;
use uuid::Uuid;

const PATH: &str = "/api/v1/community/credentials";

#[tokio::test]
async fn authenticated_reset_clears_only_the_callers_credentials_even_without_the_encryption_key() {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let mut sessions = Vec::new();
    let mut queued = Vec::new();
    for (account, username) in library
        .accounts
        .iter()
        .zip(["recovery-admin", "recovery-listener"])
    {
        sessions.push(
            hito.identity
                .login(fixture::credentials(username))
                .await
                .unwrap(),
        );
        hito.community
            .update_preferences(
                &account.actor,
                UpdateCommunityPreferences {
                    revision: 0,
                    playback_reporting: true,
                    metadata_corrections: true,
                },
            )
            .await
            .unwrap();
        let submission_id = Uuid::new_v4();
        assert_eq!(
            hito.community
                .observe_success(
                    &account.actor,
                    &account.source.id,
                    SuccessfulPlayback { submission_id }
                )
                .await
                .unwrap(),
            CommunityReportAdmission::Queued
        );
        assert!(
            hito.community
                .run_next_report_until(pending())
                .await
                .unwrap()
        );
    }
    for account in &library.accounts {
        let submission_id = Uuid::new_v4();
        queued.push(submission_id);
        assert_eq!(
            hito.community
                .observe_success(
                    &account.actor,
                    &account.source.id,
                    SuccessfulPlayback { submission_id }
                )
                .await
                .unwrap(),
            CommunityReportAdmission::Queued
        );
    }
    let before = [
        library.accounts[0].private_state(&hito).await,
        library.accounts[1].private_state(&hito).await,
    ];
    let receipts = remote.accepted_receipts().await;
    assert_eq!(receipts.len(), 2);
    let count = remote.requests().await.len();
    drop(hito);
    let key_path = library.directory.path().join("hito.plugin-key");
    std::fs::remove_file(&key_path).unwrap();
    let hito = library.open(&remote).await;
    assert!(
        !hito
            .plugins
            .settings(&library.accounts[0].actor)
            .await
            .unwrap()
            .credential_storage_ready
    );
    let app = hito_server::router(hito.clone());
    assert_eq!(reset(&app, None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reset(&app, Some(&sessions[0].token), Some(&sessions[1].user.id))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for session in sessions.iter().take(2) {
        let (status, receipt) = reset(&app, Some(&session.token), Some(&session.user.id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            receipt,
            json!({"reporters_removed": 1, "reports_cancelled": 1})
        );
        assert_eq!(
            reset(&app, Some(&session.token), Some(&session.user.id)).await,
            (
                StatusCode::OK,
                json!({"reporters_removed": 0, "reports_cancelled": 0})
            )
        );
        for (account, expected) in library.accounts.iter().zip(&before) {
            assert_eq!(&account.private_state(&hito).await, expected);
        }
        assert!(
            !key_path.exists(),
            "Community reset must not replace the unavailable shared key"
        );
        assert!(
            !hito
                .community
                .run_next_report_until(pending())
                .await
                .unwrap()
        );
        assert_eq!(remote.requests().await.len(), count);
        assert_eq!(remote.accepted_receipts().await, receipts);
    }
    for (account, queued) in library.accounts.iter().zip(queued) {
        assert_eq!(
            hito.community
                .observe_success(
                    &account.actor,
                    &account.source.id,
                    SuccessfulPlayback {
                        submission_id: queued
                    }
                )
                .await
                .unwrap(),
            CommunityReportAdmission::Ignored
        );
    }
    hito.identity.logout(&sessions[0].token).await.unwrap();
    assert_eq!(
        reset(&app, Some(&sessions[0].token), Some(&sessions[0].user.id))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(remote.requests().await.len(), count);
    library.assert_originals();
}

async fn reset(app: &Router, token: Option<&str>, account: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("DELETE")
        .uri(PATH)
        .header("origin", "http://localhost:8080")
        .header("x-hito-request", "1");
    if let Some(token) = token {
        request = request.header("cookie", format!("hito_session={token}"));
    }
    if let Some(account) = account {
        request = request.header("x-hito-account", account);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
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
