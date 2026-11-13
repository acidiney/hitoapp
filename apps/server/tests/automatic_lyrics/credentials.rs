use super::setup::Prepared;
use crate::{
    account::{self, Account},
    provider, support,
};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};

type Response = (StatusCode, HeaderMap, Value);

#[tokio::test]
async fn automatic_credential_failures_are_private_actionable_and_recover_at_every_protected_stage()
{
    let prepared = Prepared::new().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(Box::pin(exercise(&prepared))).catch_unwind(),
    )
    .await;
    let Prepared {
        library, provider, ..
    } = prepared;
    provider.finish().await;
    library.assert_original();
    let responses = match result {
        Ok(Ok(responses)) => responses,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned automatic credential workflow must settle within its deadline"),
    };
    assert_eq!(responses.len(), 4);
    let expected_document: hito_plugin_sdk::LyricsResponseV2 =
        serde_json::from_value(provider::response()).unwrap();
    let expected_document = serde_json::to_value(expected_document).unwrap();
    for (operation, failure, expected_stamp, recovery) in responses {
        assert_eq!(failure.0, StatusCode::CONFLICT, "stage {operation}");
        assert_eq!(failure.1["cache-control"], "private, no-store");
        assert_eq!(
            failure.2["code"], "plugin_credentials_required",
            "stage {operation}"
        );
        assert_eq!(failure.2["plugin_connection"], expected_stamp);
        assert_eq!(
            failure.2["error"],
            "Update this plugin connection's service credentials in Settings."
        );
        assert!(failure.2.get("document").is_none());
        assert!(failure.2.get("status").is_none());
        assert_eq!(recovery.0, StatusCode::OK);
        assert_eq!(recovery.1["cache-control"], "private, no-store");
        assert_eq!(recovery.2["status"], "ready_v2", "stage {operation}");
        assert_eq!(recovery.2["recording_id"], library.recording);
        assert_eq!(
            recovery.2["installation_id"],
            expected_stamp["installation_id"]
        );
        assert_eq!(
            recovery.2["installation_revision"],
            expected_stamp["installation_revision"]
        );
        assert_eq!(recovery.2["document"], expected_document);
        assert!(recovery.2.get("plugin_connection").is_none());
    }
}

async fn exercise(prepared: &Prepared) -> Vec<(&'static str, Response, Value, Response)> {
    let library = &prepared.library;
    let owner = &library.account;
    let app = &library.app;
    let base = prepared.path().trim_end_matches("/automatic").to_owned();
    let candidate = json!({
        "installation_id":prepared.installation["id"],
        "installation_revision":prepared.installation["revision"],
        "provider_recording_id":"owned:automatic",
    });
    let (status, review) = owner
        .request(
            app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, retained) = owner
        .request(
            app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retained["selector"]["kind"], "reviewed");
    let identities = owner.get(app, &format!("{base}/identities")).await;
    assert_eq!(identities["items"].as_array().unwrap().len(), 1);
    let (status, _) = owner.request(app, "POST", "/api/v1/users",
        json!({"username":"automatic-credential-peer", "password":"owned automatic credential peer password"})).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        app,
        "automatic-credential-peer",
        "owned automatic credential peer password",
    )
    .await;
    let settings = private_settings(prepared, owner).await;
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == prepared.installation["service_id"])
        .unwrap();
    let (status, peer_installation) = peer
        .request(
            app,
            "POST",
            "/api/v1/plugins",
            json!({
                "service_id":service["id"], "service_revision":service["revision"],
                "bearer_token":"owned-automatic-credential-peer-bearer",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(peer_installation["id"], prepared.installation["id"]);
    let peer_before = private_settings(prepared, &peer).await;
    assert_eq!(peer_before["installations"].as_array().unwrap().len(), 1);
    let before = library.snapshot().await;
    let baseline_calls = prepared.provider.calls();
    assert_eq!(
        baseline_calls,
        vec!["manifest", "metadata", "metadata", "manifest"]
    );
    let (status, listener_error) =
        support::call(app, "POST", &prepared.path(), json!({}), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(listener_error.get("plugin_connection").is_none());
    let (status, peer_error) = peer.request(app, "POST", &prepared.path(), json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(peer_error.get("plugin_connection").is_none());
    assert_eq!(prepared.provider.calls(), baseline_calls);
    let mut expected_calls = baseline_calls;
    let mut responses = Vec::new();
    let started = Instant::now();
    for (operation, prefix) in [
        ("search", vec!["search"]),
        ("metadata", vec!["search", "metadata"]),
        ("v2_descriptor", vec!["search", "metadata", "v2_descriptor"]),
        (
            "v2_lyrics",
            vec!["search", "metadata", "v2_descriptor", "v2_lyrics"],
        ),
    ] {
        prepared
            .provider
            .status(operation, StatusCode::UNAUTHORIZED);
        let failure =
            account::exchange(app, "POST", &prepared.path(), json!({}), owner.headers()).await;
        expected_calls.extend(prefix);
        assert_eq!(prepared.provider.calls(), expected_calls);
        let diagnosed = private_settings(prepared, owner).await;
        let current = diagnosed["installations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == prepared.installation["id"])
            .unwrap();
        assert_eq!(current["revision"], prepared.installation["revision"]);
        assert_eq!(current["service_id"], prepared.installation["service_id"]);
        assert_eq!(current["health"]["state"], "credentials_required");
        let mut expected_plugins = before["plugins"].clone();
        let expected_installation = expected_plugins["installations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|item| item["id"] == prepared.installation["id"])
            .unwrap();
        expected_installation["health"] =
            json!({"state":"credentials_required", "reason":null, "since":null});
        assert_eq!(diagnosed, expected_plugins);
        let mut failed_snapshot = library.snapshot().await;
        assert_eq!(failed_snapshot["plugins"], diagnosed);
        failed_snapshot["plugins"] = before["plugins"].clone();
        assert_eq!(failed_snapshot, before);
        assert_eq!(
            owner.get(app, &format!("{base}/identities")).await,
            identities
        );
        assert_eq!(private_settings(prepared, &peer).await, peer_before);
        library.assert_original();
        let encoded = serde_json::to_string(&failure.2).unwrap();
        let recording = provider::recording();
        for private in [
            recording["title"].as_str().unwrap(),
            "owned-automatic-credential-peer-bearer",
            prepared.provider.url(),
        ] {
            assert!(!encoded.contains(private));
        }
        prepared.provider.status(operation, StatusCode::OK);
        let recovery =
            account::exchange(app, "POST", &prepared.path(), json!({}), owner.headers()).await;
        expected_calls.extend(["search", "metadata", "v2_descriptor", "v2_lyrics"]);
        assert_eq!(prepared.provider.calls(), expected_calls);
        assert_eq!(library.snapshot().await, before);
        assert_eq!(
            owner.get(app, &format!("{base}/identities")).await,
            identities
        );
        assert_eq!(private_settings(prepared, &peer).await, peer_before);
        library.assert_original();
        responses.push((
            operation,
            failure,
            json!({
                "installation_id":prepared.installation["id"],
                "installation_revision":prepared.installation["revision"],
            }),
            recovery,
        ));
    }
    assert_eq!(prepared.provider.calls().len(), 30);
    assert!(started.elapsed() < Duration::from_secs(20));
    responses
}

async fn private_settings(prepared: &Prepared, account: &Account) -> Value {
    let (status, headers, settings) = account::exchange(
        &prepared.library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    settings
}
