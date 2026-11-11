use super::setup::Prepared;
use crate::account;
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn a_private_reviewed_lyric_identity_retains_a_missing_recording_until_explicit_unlink() {
    let prepared = Prepared::new().await;
    let (status, user) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"missing-lyric-peer","password":"owned missing lyric peer password"}),
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
        "missing-lyric-peer",
        "owned missing lyric peer password",
    )
    .await;
    prepared.library.hide_original();
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("/api/v1/libraries/{shared}/scan"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(
        prepared
            .library
            .services
            .storage
            .run_next_job()
            .await
            .unwrap()
    );
    let catalog = prepared
        .library
        .account
        .get(&prepared.library.app, "/api/v1/recordings")
        .await;
    assert!(
        catalog["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == prepared.library.recording && item["available"] == false)
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
    assert_eq!(
        peer.get(
            &prepared.library.app,
            &format!("{}/identities", prepared.base())
        )
        .await["items"],
        json!([])
    );
    let before = prepared.library.snapshot().await;
    let purge = format!("/api/v1/libraries/{shared}/missing-recordings");
    let (status, result) = prepared
        .library
        .account
        .request(&prepared.library.app, "DELETE", &purge, Value::Null)
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "A reviewed private lyric association must retain its missing recording instead of causing a foreign-key purge failure"
    );
    assert_eq!(
        result,
        json!({"removed":0,"kept_referenced":1,"more":false})
    );
    assert_eq!(prepared.library.snapshot().await, before);
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
    assert_eq!(
        peer.get(
            &prepared.library.app,
            &format!("{}/identities", prepared.base())
        )
        .await["items"],
        json!([])
    );
    prepared.library.assert_hidden_original();
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "DELETE",
            &format!(
                "{}/identities/{}?revision=1",
                prepared.base(),
                prepared.identity["selector"]["identity_id"]
                    .as_str()
                    .unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, result) = prepared
        .library
        .account
        .request(&prepared.library.app, "DELETE", &purge, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        result,
        json!({"removed":1,"kept_referenced":0,"more":false})
    );
    let owner_catalog = prepared
        .library
        .account
        .get(&prepared.library.app, "/api/v1/recordings")
        .await;
    let peer_catalog = peer.get(&prepared.library.app, "/api/v1/recordings").await;
    assert_eq!(owner_catalog["total"], 0);
    assert_eq!(peer_catalog["total"], 0);
    for actor in [&prepared.library.account, &peer] {
        let (status, _) = actor
            .request(
                &prepared.library.app,
                "GET",
                &format!("{}/identities", prepared.base()),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    prepared
        .provider
        .assert_calls(&["manifest", "metadata", "metadata"]);
    prepared.library.restore_original();
    prepared.provider.finish().await;
}
