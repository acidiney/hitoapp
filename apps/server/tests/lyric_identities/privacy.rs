use super::setup::Prepared;
use crate::{account, fixture, lyric_provider};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn another_account_with_the_same_local_recording_cannot_read_or_remove_private_lyric_authority()
 {
    let prepared = Prepared::new().await;
    let (status, user) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"shared-peer","password":"owned lyric shared peer password"}),
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
    let grant = format!(
        "/api/v1/libraries/{shared}/members/{}",
        user["id"].as_str().unwrap()
    );
    let peer = account::Account::login(
        &prepared.library.app,
        "shared-peer",
        "owned lyric shared peer password",
    )
    .await;
    let (status, _) = peer
        .request(
            &prepared.library.app,
            "GET",
            &format!("{}/services", prepared.base()),
            Value::Null,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "An account without the recording grant cannot list lyric services"
    );
    let (status, _) = prepared
        .library
        .account
        .request(&prepared.library.app, "PUT", &grant, Value::Null)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        peer.get(
            &prepared.library.app,
            &format!("{}/services", prepared.base())
        )
        .await["items"],
        json!([])
    );
    assert_eq!(
        peer.get(
            &prepared.library.app,
            &format!("{}/identities", prepared.base())
        )
        .await["items"],
        json!([])
    );
    for (suffix, body) in [
        (
            "candidates",
            json!({"installation_id":prepared.installation["id"],"installation_revision":prepared.installation["revision"],"query":"Explicit private search"}),
        ),
        ("identities/review", prepared.candidate.clone()),
        ("identities", prepared.accept.clone()),
        ("identities/read", prepared.read_body()),
    ] {
        let (status, result) = peer
            .request(
                &prepared.library.app,
                "POST",
                &format!("{}/{suffix}", prepared.base()),
                body,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(result.get("document").is_none() && result.get("review_stamp").is_none());
    }
    let path = format!(
        "{}/identities/{}?revision=1",
        prepared.base(),
        prepared.identity["selector"]["identity_id"]
            .as_str()
            .unwrap()
    );
    let (status, _) = peer
        .request(&prepared.library.app, "DELETE", &path, Value::Null)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let mut headers = prepared.library.account.headers();
    headers.remove("x-hito-request");
    let (status, _, _) = account::exchange(
        &prepared.library.app,
        "POST",
        &format!("{}/identities/read", prepared.base()),
        prepared.read_body(),
        headers,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut stale = prepared.read_body();
    stale["installation_revision"] = json!(2);
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities/read", prepared.base()),
            stale,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let mut stale = prepared.read_body();
    stale["selector"]["identity_revision"] = json!(2);
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities/read", prepared.base()),
            stale,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    prepared
        .provider
        .assert_calls(&["manifest", "metadata", "metadata"]);
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
    prepared.library.assert_original();
    prepared.provider.finish().await;
}

#[tokio::test]
async fn missing_or_revoked_approvals_never_disclose_a_recording_or_query_to_a_provider() {
    for capabilities in [
        json!(["metadata", "lyrics"]),
        json!(["search", "lyrics"]),
        json!(["search", "metadata"]),
    ] {
        let provider = lyric_provider::Provider::start().await;
        provider.capabilities(capabilities).await;
        let library = fixture::Library::prepare().await;
        let installation = library
            .account
            .connect(&library.app, &provider.url, "Independent lyrics", None)
            .await;
        let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
        assert_eq!(
            library
                .account
                .get(&library.app, &format!("{base}/services"))
                .await["items"],
            json!([])
        );
        for (suffix, body) in [
            (
                "candidates",
                json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"query":"Explicit private search"}),
            ),
            (
                "identities/review",
                json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"provider_recording_id":"independent:recording"}),
            ),
        ] {
            let (status, _) = library
                .account
                .request(&library.app, "POST", &format!("{base}/{suffix}"), body)
                .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        }
        provider.assert_calls(&["manifest"]);
        library.assert_original();
        provider.finish().await;
    }
    let prepared = Prepared::new().await;
    prepared
        .provider
        .capabilities(json!(["search", "metadata"]))
        .await;
    let refresh = format!(
        "/api/v1/plugins/{}/refresh",
        prepared.installation["id"].as_str().unwrap()
    );
    let (status, review) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &refresh,
            json!({"revision":prepared.installation["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["kind"], "review_required");
    let (status,updated)=prepared.library.account.request(&prepared.library.app,"POST",&refresh,json!({"revision":prepared.installation["revision"],"accept_manifest_digest":review["digest"]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["kind"], "updated");
    let listed = prepared
        .library
        .account
        .get(
            &prepared.library.app,
            &format!("{}/identities", prepared.base()),
        )
        .await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["selectable"], false);
    let mut body = prepared.read_body();
    body["installation_revision"] = updated["installation"]["revision"].clone();
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities/read", prepared.base()),
            body,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        prepared
            .library
            .account
            .get(
                &prepared.library.app,
                &format!("{}/services", prepared.base())
            )
            .await["items"],
        json!([])
    );
    prepared
        .provider
        .assert_calls(&["manifest", "metadata", "metadata", "manifest", "manifest"]);
    prepared.library.assert_original();
    prepared.provider.finish().await;
}
