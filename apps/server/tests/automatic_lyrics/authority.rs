use super::setup::{Prepared, commit_metadata};
use crate::{account, lrclib};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Copy)]
enum Change {
    GrantEpoch,
    MetadataEpoch,
    Installation,
    Service,
    Session,
    PinSetup,
    PinPolicy,
    PinLock,
}

async fn set_pin(prepared: &Prepared) {
    let library = &prepared.library;
    let (status, _) = library.account.request(&library.app, "PUT", "/api/v1/account/pin", json!({
        "password":"owned artist review administrator password","pin":"4271","idle_minutes":0,
    })).await;
    assert_eq!(status, StatusCode::OK);
}

async fn change(prepared: &Prepared, change: Change) -> Vec<StatusCode> {
    let library = &prepared.library;
    let actor = &library.account;
    let app = &library.app;
    match change {
        Change::GrantEpoch => {
            let libraries = actor.get(app, "/api/v1/libraries").await;
            let shared = libraries
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["personal"] == false)
                .unwrap()["id"]
                .as_str()
                .unwrap();
            for method in ["DELETE", "PUT"] {
                let (status, _) = actor
                    .request(
                        app,
                        method,
                        &format!("/api/v1/libraries/{shared}/members/{}", actor.id),
                        Value::Null,
                    )
                    .await;
                assert_eq!(status, StatusCode::NO_CONTENT);
            }
            vec![StatusCode::NOT_FOUND, StatusCode::CONFLICT]
        }
        Change::MetadataEpoch => {
            commit_metadata(library, "Owned temporarily changed title", lrclib::ARTIST).await;
            commit_metadata(library, lrclib::TITLE, lrclib::ARTIST).await;
            vec![StatusCode::CONFLICT]
        }
        Change::Installation => {
            let (status, _) = actor
                .request(
                    app,
                    "DELETE",
                    &format!(
                        "/api/v1/plugins/{}/credential",
                        prepared.installation["id"].as_str().unwrap()
                    ),
                    json!({"revision":prepared.installation["revision"]}),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            vec![StatusCode::CONFLICT]
        }
        Change::Service => {
            let (status, _) = actor
                .request(
                    app,
                    "DELETE",
                    &format!(
                        "/api/v1/plugins/services/{}",
                        prepared.installation["service_id"].as_str().unwrap()
                    ),
                    json!({"revision":1}),
                )
                .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            vec![StatusCode::NOT_FOUND]
        }
        Change::Session => {
            let (status, _) = actor
                .request(app, "POST", "/api/v1/logout", Value::Null)
                .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            vec![StatusCode::UNAUTHORIZED]
        }
        Change::PinSetup => {
            set_pin(prepared).await;
            vec![StatusCode::CONFLICT]
        }
        Change::PinPolicy => {
            let (status, _) = actor.request(app, "PUT", "/api/v1/account/pin", json!({"password":"owned artist review administrator password","pin":null,"idle_minutes":5})).await;
            assert_eq!(status, StatusCode::OK);
            vec![StatusCode::CONFLICT]
        }
        Change::PinLock => {
            let (status, _) = actor
                .request(app, "POST", "/api/v1/account/lock", Value::Null)
                .await;
            assert_eq!(status, StatusCode::OK);
            vec![StatusCode::LOCKED]
        }
    }
}

#[tokio::test]
async fn held_automatic_work_rejects_changed_session_grant_metadata_installation_service_and_current_pin_policy()
 {
    for (change_kind, hop) in [
        (Change::GrantEpoch, "search"),
        (Change::MetadataEpoch, "metadata"),
        (Change::MetadataEpoch, "v2_lyrics"),
        (Change::Installation, "v2_descriptor"),
        (Change::Service, "v2_lyrics"),
        (Change::Session, "search"),
        (Change::PinSetup, "metadata"),
        (Change::PinPolicy, "v2_lyrics"),
        (Change::PinLock, "v2_descriptor"),
    ] {
        let prepared = Prepared::new().await;
        if matches!(change_kind, Change::PinPolicy | Change::PinLock) {
            set_pin(&prepared).await;
        }
        let gate = prepared.provider.hold(hop);
        let app = prepared.library.app.clone();
        let path = prepared.path();
        let headers = prepared.library.account.headers();
        let mut task = tokio::spawn(async move {
            account::exchange(&app, "POST", &path, json!({}), headers).await
        });
        tokio::time::timeout(Duration::from_secs(3), gate.entered())
            .await
            .unwrap();
        let expected = change(&prepared, change_kind).await;
        let completed = tokio::time::timeout(Duration::from_secs(3), &mut task).await;
        let rejected = matches!(&completed, Ok(Ok((status, _, value))) if expected.contains(status) && value.get("document").is_none());
        if completed.is_err() {
            task.abort();
            let _ = task.await;
        }
        tokio::time::timeout(Duration::from_secs(2), gate.retired())
            .await
            .unwrap();
        assert!(!gate.released());
        assert!(
            rejected,
            "Held automatic work must reject changed authority before any document is published"
        );
        prepared.finish().await;
    }
}
