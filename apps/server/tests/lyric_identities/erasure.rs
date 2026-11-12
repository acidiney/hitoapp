use super::setup::Prepared;
use crate::account;
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn erasing_an_account_removes_its_lyric_authority_and_preserves_the_shared_recording_and_peer()
 {
    let prepared = Prepared::new().await;
    let before = prepared.library.snapshot().await;
    let (status, user) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"erased-lyric-peer","password":"owned erased lyric peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let libraries = prepared
        .library
        .account
        .get(&prepared.library.app, "/api/v1/libraries")
        .await;
    let shared = libraries
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["personal"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "PUT",
            &format!(
                "/api/v1/libraries/{shared}/members/{}",
                user["id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let peer = account::Account::login(
        &prepared.library.app,
        "erased-lyric-peer",
        "owned erased lyric peer password",
    )
    .await;
    let (status, installation) = peer
        .request(
            &prepared.library.app,
            "POST",
            "/api/v1/plugins",
            json!({"service_id":prepared.installation["service_id"],"service_revision":1}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let candidate = json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"provider_recording_id":"independent:recording"});
    let (status, review) = peer
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities/review", prepared.base()),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, identity) = peer
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities", prepared.base()),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(identity["selector"], prepared.identity["selector"]);
    assert_eq!(
        peer.get(
            &prepared.library.app,
            &format!("{}/identities", prepared.base())
        )
        .await["items"],
        json!([identity])
    );
    assert_eq!(
        prepared
            .library
            .account
            .get(
                &prepared.library.app,
                &format!("{}/identities", prepared.base())
            )
            .await["items"],
        json!([prepared.identity])
    );
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "DELETE",
            &format!("/api/v1/users/{}", peer.id),
            json!({"username":"erased-lyric-peer"}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "Account erasure must delete private shared-recording lyric rows before their installation and user parents"
    );
    let (status, _) = peer
        .request(
            &prepared.library.app,
            "GET",
            &format!("{}/identities", prepared.base()),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(prepared.library.snapshot().await, before);
    assert_eq!(prepared.read().await["status"], "ready_v2");
    prepared.library.assert_original();
    prepared.provider.assert_calls(&[
        "manifest",
        "metadata",
        "metadata",
        "manifest",
        "metadata",
        "metadata",
        "v2_descriptor",
        "v2_lyrics",
    ]);
    prepared.provider.finish().await;
}
