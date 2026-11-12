use super::setup::{Prepared, commit_metadata};
use crate::{fixture, lrclib, provider};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn local_no_service_and_insufficient_metadata_states_do_not_disclose_any_query() {
    let provider = provider::Provider::start().await;
    let library = fixture::Library::prepare().await;
    let path = format!("/api/v1/recordings/{}/lyrics/automatic", library.recording);
    let before = library.snapshot().await;
    let (status, response) = library
        .account
        .request(&library.app, "POST", &path, json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response, json!({"status":"no_services"}));
    assert_eq!(library.snapshot().await, before);
    assert!(provider.calls().is_empty());
    library
        .account
        .connect(
            &library.app,
            provider.url(),
            "Owned automatic local control",
            None,
        )
        .await;
    let before = library.snapshot().await;
    let (status, response) = library
        .account
        .request(&library.app, "POST", &path, json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response, json!({"status":"insufficient_metadata"}));
    assert_eq!(provider.calls(), vec!["manifest"]);
    assert_eq!(library.snapshot().await, before);
    commit_metadata(&library, lrclib::TITLE, lrclib::ARTIST).await;
    let (status, response) = library
        .account
        .request(&library.app, "POST", &path, json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["status"], "ready_v2");
    library.assert_original();
    provider.finish().await;
}

#[tokio::test]
async fn automatic_read_uses_only_the_first_approved_connection_in_the_existing_service_order() {
    let prepared = Prepared::new().await;
    let other = provider::Provider::start().await;
    other.reply("manifest", json!({"id":"org.hito.owned-second-lyrics","name":"Owned second lyrics","protocol_version":1,"capabilities":["search","metadata","lyrics"]}));
    let installation = prepared
        .library
        .account
        .connect(
            &prepared.library.app,
            other.url(),
            "Owned second service",
            None,
        )
        .await;
    let services = prepared
        .library
        .account
        .get(
            &prepared.library.app,
            &prepared.path().replace("/automatic", "/services"),
        )
        .await;
    assert_eq!(services["items"].as_array().unwrap().len(), 2);
    let (first, untouched) =
        if services["items"][0]["installation_id"] == prepared.installation["id"] {
            (&prepared.provider, &other)
        } else {
            assert_eq!(services["items"][0]["installation_id"], installation["id"]);
            (&other, &prepared.provider)
        };
    let before = prepared.library.snapshot().await;
    first.reply("search", json!({"items":[],"next_cursor":null}));
    assert_eq!(prepared.read().await, json!({"status":"not_found"}));
    first.status("search", StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(prepared.read().await, json!({"status":"unavailable"}));
    assert_eq!(first.calls(), vec!["manifest", "search", "search"]);
    assert_eq!(untouched.calls(), vec!["manifest"]);
    assert_eq!(prepared.library.snapshot().await, before);
    other.finish().await;
    prepared.finish().await;
}

#[tokio::test]
async fn a_shared_library_grant_does_not_disclose_another_accounts_lyric_connection_or_personal_metadata()
 {
    let prepared = Prepared::new().await;
    let library = &prepared.library;
    let (status, user) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"automatic-peer","password":"owned automatic peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = crate::account::Account::login(
        &library.app,
        "automatic-peer",
        "owned automatic peer password",
    )
    .await;
    let (status, _) = peer
        .request(&library.app, "POST", &prepared.path(), json!({}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let libraries = library.account.get(&library.app, "/api/v1/libraries").await;
    let shared = libraries
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["personal"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "PUT",
            &format!(
                "/api/v1/libraries/{shared}/members/{}",
                user["id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let before = library.snapshot().await;
    let (status, response) = peer
        .request(&library.app, "POST", &prepared.path(), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response, json!({"status":"no_services"}));
    assert_eq!(prepared.provider.calls(), vec!["manifest"]);
    assert_eq!(library.snapshot().await, before);
    prepared.finish().await;
}
